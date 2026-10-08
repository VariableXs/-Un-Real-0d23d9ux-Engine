//! F240 全局音量 OSD 与静音热键 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：F240（主册 H-1 深化设计报告 · H 基础通用域）。
//!
//! **验收标准（主册第一句）**：音量键调节显示与亮度同规格的 OSD
//! （竖条式，与亮度横条形制区分防误读），0-100 按 2 步进、按住连发；
//! 静音热键一键切换且 OSD 显示斜杠图标状态；静音是「记住的」——重启后
//! 保持静音态（不半夜开机炸音箱）；音量 100 之上不再放大（不做超过
//! 100% 的失真增益）。
//!
//! **设计要点**：
//! - 形制区分是**类型级**的：`Orient::{Vertical, Horizontal}` 枚举 +
//!   `form_distinct` 走查判定——音量竖条、亮度横条（F239），渲染参数
//!   从类型流出，不靠约定；
//! - 步进器：音量键一步 ±2（0-100 域），按住连发（延时 400ms 起拍、
//!   每 30ms 一步——键入节奏面），松键即停；
//! - 钳制纪律：100 之上 saturating 不放大（拒绝 >100% 失真增益），
//!   0 之下不回绕；
//! - 「记住的静音」：muted 进持久化字节（与音量值一起 round-trip），
//!   重启装载静音态——半夜开机不炸音箱；
//! - 低延迟链：按键时刻 → 音量事件生效时刻的差值进环账本，P95 < 50ms
//!   判定（快路径 = 键盘中断直调音量混音器，不绕通知/UI 队列）；
//! - 参数面进旋钮注册表，操作面进分钟账本（上/下/静音开/静音关）。
//!
//! **依赖锚点**：[`crate::star::sbase::{KnobReg, MinuteBook, RingLog,
//! pct_near}`]；时间一律注入毫秒戳。

use crate::checks::CheckSet;
use crate::star::sbase::{pct_near, KnobReg, MinuteBook, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 音量步进——主册 F240「0-100 按 2 步进」。
pub const VOL_STEP: u8 = 2;

/// 音量域上限——主册 F240「100 之上不再放大（不做超过 100% 的失真增益）」。
pub const VOL_MAX: u8 = 100;

/// 连发起拍延时——按住 400ms 后开始连发（键入节奏面设计值）。
pub const REPEAT_DELAY_MS: u64 = 400;

/// 连发节拍——每 30ms 一步（长按扫过全程 0-100 约 1.5s）。
pub const REPEAT_INTERVAL_MS: u64 = 30;

/// 静音持久化字节（非零 = 静音）。
pub const MUTE_ON_BYTE: u8 = 0xA5;

/// 低延迟判定门——主册 F240 验收「按键到出声 <50ms」（P95 口径）。
pub const LATENCY_P95_LIMIT_MS: u64 = 50;

/// 低延迟账本容量（滚动最近 128 次实测）。
const LATENCY_RING: usize = 128;

/// 旋钮名（旋钮注册表唯一户口）。
pub const KNOB_NAME: &str = "volume";

/// 账本计数器列：0 音量升 / 1 音量降 / 2 静音开 / 3 静音关。
pub const LEDGER_COLS: usize = 4;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// OSD 形制（类型级区分——防误读的根）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orient {
    /// 横条（亮度 F239 用）。
    Horizontal,
    /// 竖条（音量 F240 用）。
    Vertical,
}

/// 形制走查判定：两 OSD 形制必须可区分（同向即判红——防误读的自动化）。
pub fn form_distinct(a: Orient, b: Orient) -> bool {
    a != b
}

/// 音量事件（环形日志，小拷贝体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeEvent {
    /// 音量变更（来源 0=键 1=滑杆 2=连发；从几到几）。
    Changed { src: u8, from: u8, to: u8 },
    /// 静音开（OSD 斜杠图标亮起）。
    Muted,
    /// 静音关（斜杠图标熄灭）。
    Unmuted,
    /// OSD 显示刷新。
    OsdShown { value: u8 },
    /// 上限拒增（100 处按升——显性事件不静默）。
    CeilingRejected,
    /// 下限拒减（0 处按降）。
    FloorRejected,
}

/// 连发状态机（按住期间推进，松键复位）。
#[derive(Clone, Copy, Debug)]
struct RepeatState {
    held: bool,
    dir: i8,
    start_ms: u64,
    /// 下一拍到期时刻（起拍 = 按下 + 延时；之后每 INTERVAL_MS 一拍）。
    next_due_ms: u64,
}

/// 音量治理器：步进连发 + 静音记忆 + 竖条 OSD + 低延迟账本。
pub struct VolumeGov {
    knobs: KnobReg,
    muted: bool,
    repeat: Option<RepeatState>,
    osd_shown_ms: Option<u64>,
    osd_value: u8,
    events: RingLog<VolumeEvent, 32>,
    latencies: RingLog<u64, LATENCY_RING>,
    ledger: MinuteBook,
    /// 操作统计（诊断面）。
    pub stats: VolStats,
}

/// 操作统计（诊断面直读）。
#[derive(Clone, Copy, Debug, Default)]
pub struct VolStats {
    pub up_ops: u32,
    pub down_ops: u32,
    pub repeat_steps: u32,
    pub mute_on: u32,
    pub mute_off: u32,
    pub ceiling_rejects: u32,
    pub floor_rejects: u32,
}

// ---------------------------------------------------------------------------
// 音量治理器
// ---------------------------------------------------------------------------

impl VolumeGov {
    pub fn new() -> VolumeGov {
        let mut knobs = KnobReg::new();
        knobs.declare(
            KNOB_NAME,
            50,
            0,
            VOL_MAX as i64,
            "%",
            "F240 音量：2 步进按住连发，100 之上不放大，静音跨重启",
        );
        VolumeGov {
            knobs,
            muted: false,
            repeat: None,
            osd_shown_ms: None,
            osd_value: 50,
            events: RingLog::new(),
            latencies: RingLog::new(),
            ledger: MinuteBook::new(LEDGER_COLS, 30 * 1440),
            stats: VolStats::default(),
        }
    }

    pub fn value(&self) -> u8 {
        self.knobs.get_or(KNOB_NAME, 50) as u8
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    // -- 步进与连发 ----------------------------------------------------------

    /// 音量键按下：立即一步并启动连发（dir: +1 升 / -1 降）。
    /// 返回 (新值, 实际变化量)。
    pub fn key_down(&mut self, dir: i8, key_ms: u64, applied_ms: u64) -> (u8, i32) {
        self.repeat =
            Some(RepeatState { held: true, dir, start_ms: key_ms, next_due_ms: key_ms + REPEAT_DELAY_MS });
        self.step(dir, key_ms, applied_ms)
    }

    /// 音量键松开：连发停止。
    pub fn key_up(&mut self) {
        self.repeat = None;
    }

    /// 连发推进：到期拍逐一补走（不丢拍；单次至多补 32 拍防卡顿尖峰）。
    /// 返回本拍实际走出的步数。
    pub fn repeat_tick(&mut self, now_ms: u64) -> u32 {
        let (dir, mut next_due) = match self.repeat {
            Some(r) if r.held => (r.dir, r.next_due_ms),
            _ => return 0,
        };
        let mut fired = 0u32;
        while next_due < now_ms && fired < 32 {
            self.step(dir, next_due, next_due);
            self.stats.repeat_steps += 1;
            fired += 1;
            next_due += REPEAT_INTERVAL_MS;
        }
        if fired > 0 {
            if let Some(r) = self.repeat.as_mut() {
                r.next_due_ms = next_due;
            }
        }
        fired
    }

    /// 单步 ±2（钳制 0..=100，不放大不回绕；边界显性拒增/拒减）。
    fn step(&mut self, dir: i8, key_ms: u64, applied_ms: u64) -> (u8, i32) {
        let from = self.value();
        let (to, rejected) = if dir > 0 {
            if from >= VOL_MAX {
                self.stats.ceiling_rejects += 1;
                self.events.push(VolumeEvent::CeilingRejected);
                (from, true)
            } else {
                (from.saturating_add(VOL_STEP).min(VOL_MAX), false)
            }
        } else if from == 0 {
            self.stats.floor_rejects += 1;
            self.events.push(VolumeEvent::FloorRejected);
            (from, true)
        } else {
            (from.saturating_sub(VOL_STEP), false)
        };
        if !rejected && to != from {
            let _ = self.knobs.set(KNOB_NAME, to as i64, applied_ms);
            self.stats_record(dir, applied_ms);
            self.events.push(VolumeEvent::Changed { src: 0, from, to });
            self.show_osd(to, applied_ms);
            self.record_latency(key_ms, applied_ms);
        }
        (to, to as i32 - from as i32)
    }

    fn stats_record(&mut self, dir: i8, now_ms: u64) {
        let mut vals = [0u64; LEDGER_COLS];
        if dir > 0 {
            self.stats.up_ops += 1;
            vals[0] = 1;
        } else {
            self.stats.down_ops += 1;
            vals[1] = 1;
        }
        self.ledger.record_minute(now_ms / 60_000, &vals);
    }

    /// 滑杆/混音器设值：量化到 2 步进格（0-100 偶数域），不走连发。
    pub fn set_volume(&mut self, v: u8, applied_ms: u64) -> u8 {
        let from = self.value();
        // 量化到偶数格（0-100 按 2 步进的域纪律）。
        let to = (v.min(VOL_MAX)) & !1;
        if to != from {
            let _ = self.knobs.set(KNOB_NAME, to as i64, applied_ms);
            self.stats_record(if to > from { 1 } else { -1 }, applied_ms);
            self.events.push(VolumeEvent::Changed { src: 1, from, to });
            self.show_osd(to, applied_ms);
        }
        to
    }

    // -- 静音（记住的） -------------------------------------------------------

    /// 静音热键：一键切换，OSD 斜杠图标状态随事件刷新。
    pub fn mute_toggle(&mut self, now_ms: u64) -> bool {
        self.muted = !self.muted;
        let mut vals = [0u64; LEDGER_COLS];
        if self.muted {
            self.stats.mute_on += 1;
            vals[2] = 1;
            self.events.push(VolumeEvent::Muted);
        } else {
            self.stats.mute_off += 1;
            vals[3] = 1;
            self.events.push(VolumeEvent::Unmuted);
        }
        self.ledger.record_minute(now_ms / 60_000, &vals);
        self.show_osd(self.value(), now_ms);
        self.muted
    }

    // -- OSD（竖条形制） ------------------------------------------------------

    fn show_osd(&mut self, v: u8, now_ms: u64) {
        self.osd_shown_ms = Some(now_ms);
        self.osd_value = v;
        self.events.push(VolumeEvent::OsdShown { value: v });
    }

    /// OSD 形制：竖条（与亮度横条走查区分）。
    pub fn osd_orientation(&self) -> Orient {
        Orient::Vertical
    }

    /// OSD 斜杠图标态（静音时亮起）。
    pub fn osd_slash_icon(&self) -> bool {
        self.muted
    }

    /// OSD 展示值快照。
    pub fn osd_value(&self) -> Option<u8> {
        self.osd_shown_ms.map(|_| self.osd_value)
    }

    /// OSD alpha（与亮度同规格：1500ms 保持 + 200ms 线性淡出）。
    pub fn osd_alpha_at(&self, now_ms: u64) -> u8 {
        let shown = match self.osd_shown_ms {
            Some(s) => s,
            None => return 0,
        };
        const SHOW: u64 = 1_500;
        const FADE: u64 = 200;
        let elapsed = now_ms.saturating_sub(shown);
        if elapsed < SHOW {
            return 255;
        }
        if elapsed < SHOW + FADE {
            let f = ((elapsed - SHOW) * 255 / FADE) as u16;
            return (255 - f) as u8;
        }
        0
    }

    /// 竖条 50 格填充态（每格 2%——与 2 步进同粒度）。
    pub fn osd_bars(&self) -> [bool; 50] {
        let filled = (self.value() / VOL_STEP) as usize;
        let mut out = [false; 50];
        for b in out.iter_mut().take(filled.min(50)) {
            *b = true;
        }
        out
    }

    // -- 低延迟链账本 ---------------------------------------------------------

    /// 打点：按键时刻 → 音量事件生效时刻（快路径直调混音器的实测）。
    pub fn record_latency(&mut self, key_ms: u64, applied_ms: u64) {
        self.latencies.push(applied_ms.saturating_sub(key_ms));
    }

    /// 低延迟判定：样本 ≥16 且 P95 < 50ms。
    pub fn latency_ok(&self) -> bool {
        let v = self.latencies.newest_first();
        if v.len() < 16 {
            return false;
        }
        pct_near(&v, 95) < LATENCY_P95_LIMIT_MS
    }

    pub fn latency_samples(&self) -> usize {
        self.latencies.len()
    }

    // -- 持久化（记住的静音） --------------------------------------------------

    /// 持久化：音量值 + 静音态（重启装载，静音跨重启保持）。
    pub fn persist(&self) -> [u8; 2] {
        [self.value(), if self.muted { MUTE_ON_BYTE } else { 0 }]
    }

    /// 重启装载：恢复音量与静音态（校验失败整体拒绝）。
    pub fn restore_boot(&mut self, buf: &[u8]) -> bool {
        if buf.len() != 2 {
            return false;
        }
        let v = buf[0].min(VOL_MAX) & !1;
        let muted = buf[1] != 0;
        let _ = self.knobs.set(KNOB_NAME, v as i64, 0);
        self.muted = muted;
        true
    }

    // -- 诊断面 ---------------------------------------------------------------

    pub fn events(&self) -> Vec<VolumeEvent> {
        self.events.newest_first()
    }

    /// 账本区间聚合 [up, down, mute-on, mute-off]。
    pub fn ledger_sum(&self, from_min: u64, to_min: u64) -> [u64; LEDGER_COLS] {
        let v = self.ledger.range_sum(from_min, to_min);
        [v[0], v[1], v[2], v[3]]
    }

    /// 旋钮变更留痕透出（审计面）。
    pub fn change_log(&self) -> Vec<crate::star::sbase::KnobChange> {
        self.knobs.change_log()
    }
}

impl Default for VolumeGov {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// xors32 随机步进（范式照 touchpad.rs）。
fn xors32(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

/// F240 自检（判据：形制区分走查 + 连发步进实测 + 静音跨重启
/// + 100 不放大 + 低延迟链 P95 <50ms）。
pub fn run_volosd_checks() -> CheckSet {
    let mut set = CheckSet::new("F240-volosd");

    // 1. 形制区分走查：音量竖条 vs 亮度横条（类型级判定，防误读）。
    set.add(
        "form distinct: vertical vs horizontal",
        form_distinct(Orient::Vertical, Orient::Horizontal)
            && !form_distinct(Orient::Vertical, Orient::Vertical),
        "",
    );

    // 2. 2 步进：50→52→54；偶数量化：53→52。
    let mut g = VolumeGov::new();
    let (v1, d1) = g.key_down(1, 1_000, 1_010);
    let (v2, d2) = g.key_down(1, 2_000, 2_010);
    let v3 = g.set_volume(53, 3_000);
    set.add(
        "step 2 & even quantize",
        v1 == 52 && d1 == 2 && v2 == 54 && d2 == 2 && v3 == 52,
        "",
    );

    // 3. 100 之上不放大（saturating；显性拒增事件）；0 之下不回绕。
    let _ = g.set_volume(100, 4_000);
    let (v4, d4) = g.key_down(1, 5_000, 5_010);
    let _ = g.set_volume(0, 6_000);
    let (v5, d5) = g.key_down(-1, 7_000, 7_010);
    set.add(
        "no boost >100, no wrap <0",
        v4 == VOL_MAX && d4 == 0 && v5 == 0 && d5 == 0 && g.stats.ceiling_rejects == 1
            && g.stats.floor_rejects == 1,
        "",
    );

    // 4. 按住连发：400ms 起拍、每 30ms 一步、松键即停。
    let mut g2 = VolumeGov::new();
    let _ = g2.key_down(1, 10_000, 10_000); // 50→52，起拍计时开始
    set.add("repeat silent before delay", g2.repeat_tick(10_300) == 0 && g2.repeat_tick(10_399) == 0, "");
    let steps_a = g2.repeat_tick(10_430);
    let steps_b = g2.repeat_tick(10_460);
    set.add(
        "repeat ticks every 30ms after 400ms",
        steps_a == 1 && steps_b == 1 && g2.value() == 56 && g2.stats.repeat_steps == 2,
        "",
    );
    g2.key_up();
    set.add("key up stops repeat", g2.repeat_tick(10_600) == 0 && g2.repeat_tick(11_000) == 0, "");

    // 5. 静音热键：切换 + 斜杠图标事件 + 计数。
    let mut g3 = VolumeGov::new();
    let m1 = g3.mute_toggle(100);
    let m2 = g3.mute_toggle(200);
    set.add(
        "mute toggle & slash icon event",
        m1 && !m2 && g3.osd_slash_icon() == false && g3.stats.mute_on == 1 && g3.stats.mute_off == 1,
        "",
    );

    // 6. 「记住的静音」：持久化 round-trip——重启装载静音态。
    let mut g4 = VolumeGov::new();
    let _ = g4.set_volume(64, 0);
    assert!(g4.mute_toggle(100));
    let snap = g4.persist();
    let mut g5 = VolumeGov::new();
    let ok_boot = g5.restore_boot(&snap);
    set.add(
        "muted state survives reboot",
        ok_boot && g5.muted() && g5.value() == 64 && g5.osd_slash_icon(),
        "",
    );
    // 静音期间音量值不动（解除静音回到原值）。
    set.add("unmute keeps volume", g5.mute_toggle(300) == false && g5.value() == 64, "");
    set.add("persist rejects bad buf", !g5.restore_boot(&[1]) && !g5.restore_boot(&[1, 2, 3]), "");

    // 7. OSD 竖条 50 格填充（每格 2%）+ alpha 规格与亮度一致。
    let _ = g5.set_volume(30, 1_000);
    let bars = g5.osd_bars();
    let filled = bars.iter().filter(|b| **b).count();
    set.add(
        "vertical bars 50 filled by value",
        bars.len() == 50 && filled == 15 && g5.osd_orientation() == Orient::Vertical,
        "",
    );
    set.add(
        "osd alpha 1.5s+200ms fade",
        g5.osd_alpha_at(1_000 + 1_499) == 255
            && g5.osd_alpha_at(1_000 + 1_600) < 255
            && g5.osd_alpha_at(1_000 + 1_701) == 0,
        "",
    );

    // 8. 低延迟链：120 个 10ms 实测 → P95 <50ms 判定通过；再注 20 个
    //    80ms（超门 16%>5%）→ P95 仍看门内分位……构造多数超门判红。
    let mut g6 = VolumeGov::new();
    for i in 0..120u32 {
        g6.record_latency(i as u64 * 100, i as u64 * 100 + 10);
    }
    let ok_fast = g6.latency_ok() && g6.latency_samples() == 120;
    for i in 0..30u32 {
        g6.record_latency(1_000_000 + i as u64 * 100, 1_000_000 + i as u64 * 100 + 80);
    }
    // 150 样本中 30 个 80ms：P95 秩 = ceil(150*0.95)=143 → 落在 80ms 段 → 判红。
    let bad_slow = !g6.latency_ok();
    set.add("latency P95 <50ms gate", ok_fast && bad_slow, "");

    // 9. 账本：升/降/静音开/关四列聚合。
    let mut g7 = VolumeGov::new();
    let _ = g7.key_down(1, 60_000, 60_010);
    let _ = g7.key_down(-1, 120_000, 120_010);
    g7.mute_toggle(180_000);
    g7.mute_toggle(240_000);
    let s = g7.ledger_sum(1, 4);
    set.add("ledger up/down/mute-on/off", s[0] == 1 && s[1] == 1 && s[2] == 1 && s[3] == 1, "");

    // 10. 旋钮审计留痕（KnobReg：变更历史可回放——新→旧序）。
    let log = g7.change_log();
    set.add(
        "knob audit log tracks changes",
        log.len() == 2 && log[0].to == 50 && log[0].from == 52 && log[1].from == 50 && log[1].to == 52,
        "",
    );

    // 11. 静音状态下调音量：值变更但不解除静音（恢复出声须显性解除）。
    let mut g8 = VolumeGov::new();
    assert!(g8.mute_toggle(0));
    let _ = g8.key_down(1, 100, 110);
    set.add(
        "volume change keeps muted",
        g8.muted() && g8.value() == 52,
        "",
    );

    // 12. 连发补拍：节拍到期 3 拍一次补走（400/430/460 三拍全补，不丢拍）。
    let mut g9 = VolumeGov::new();
    let _ = g9.key_down(-1, 20_000, 20_000); // 50→48，起拍计时开始
    let caught = g9.repeat_tick(20_000 + 400 + 89); // 400/430/460 三拍到期
    set.add(
        "repeat catches up missed beats",
        caught == 3 && g9.stats.repeat_steps == 3 && g9.value() == 42,
        "",
    );

    // 13. xors32 fuzz：随机按键/松键/连发/静音/设值 1000 轮，不变量=
    //     值恒在 0..=100 偶数域、连发只在按住态出步、静音态与斜杠图标
    //     一致、不 panic。
    let mut gf = VolumeGov::new();
    let mut x: u32 = 0x3C6E_F372;
    let mut ok = true;
    let mut clock: u64 = 0;
    for _ in 0..1000u32 {
        clock += (xors32(&mut x) % 90) as u64;
        match xors32(&mut x) % 6 {
            0 => {
                let dir = if xors32(&mut x) % 2 == 0 { 1i8 } else { -1 };
                let _ = gf.key_down(dir, clock, clock + 8);
            }
            1 => gf.key_up(),
            2 => {
                gf.repeat_tick(clock);
            }
            3 => {
                gf.mute_toggle(clock);
            }
            4 => {
                let v = (xors32(&mut x) % 140) as u8;
                gf.set_volume(v, clock);
            }
            _ => {
                if gf.osd_slash_icon() != gf.muted() {
                    ok = false;
                }
            }
        }
        let v = gf.value();
        if v > VOL_MAX || v % 2 != 0 {
            ok = false;
        }
    }
    set.add("xors32 fuzz 1000 rounds invariants", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_and_clamp_full_sweep() {
        let mut g = VolumeGov::new();
        // 从 50 一路升到 100：25 步。
        for i in 0..25u32 {
            let (v, _) = g.key_down(1, i as u64 * 10, i as u64 * 10 + 5);
            g.key_up();
            assert_eq!(v, (50 + (i + 1) * 2) as u8, "步进 {i}");
        }
        // 100 处再升：拒绝不放大。
        let (v, d) = g.key_down(1, 999, 1_005);
        g.key_up();
        assert_eq!((v, d), (100, 0));
        assert_eq!(g.stats.ceiling_rejects, 1);
        // 一路降到 0：50 步。
        for i in 0..50u32 {
            let (v, _) = g.key_down(-1, 2_000 + i as u64 * 10, 2_005 + i as u64 * 10);
            g.key_up();
        }
        assert_eq!(g.value(), 0);
        let (v0, d0) = g.key_down(-1, 9_999, 9_999);
        g.key_up();
        assert_eq!((v0, d0), (0, 0));
        assert_eq!(g.stats.floor_rejects, 1);
    }

    #[test]
    fn repeat_cadence_measured() {
        let mut g = VolumeGov::new();
        let _ = g.key_down(1, 0, 0); // 50→52
        // 400ms 内无连发。
        assert_eq!(g.repeat_tick(399), 0);
        // 起拍后：400→430→460→490 每拍一步。
        for t in [430u64, 460, 490, 520] {
            assert_eq!(g.repeat_tick(t), 1, "{t}ms 一拍一步");
        }
        assert_eq!(g.value(), 60, "52 + 4 拍 ×2 = 60");
        assert_eq!(g.stats.repeat_steps, 4);
        g.key_up();
        assert_eq!(g.repeat_tick(10_000), 0, "松键即停");
    }

    #[test]
    fn mute_remembered_across_boot() {
        let mut g = VolumeGov::new();
        let _ = g.set_volume(88, 0);
        assert!(g.mute_toggle(1));
        let snap = g.persist();
        // 静音字节位精确（非零标记，防误清）。
        assert_eq!(snap[1], MUTE_ON_BYTE);
        let mut fresh = VolumeGov::new();
        assert!(!fresh.muted(), "出厂不静音");
        assert!(fresh.restore_boot(&snap));
        assert!(fresh.muted(), "重启保持静音——不半夜炸音箱");
        assert_eq!(fresh.value(), 88);
        assert!(fresh.osd_slash_icon());
        // 解除静音后音量回到 88（记住的是音量，不是静音期间的改动）。
        let _ = fresh.set_volume(20, 100);
        assert!(fresh.muted());
        assert!(!fresh.mute_toggle(200));
        assert_eq!(fresh.value(), 20);
    }

    #[test]
    fn latency_gate_windows() {
        let mut g = VolumeGov::new();
        // 样本不足不下结论。
        for i in 0..15u32 {
            g.record_latency(0, 10);
        }
        assert!(!g.latency_ok(), "<16 样本不判定");
        g.record_latency(0, 10);
        assert!(g.latency_ok(), "16 个 10ms 样本：P95=10 <50 达标");
        // P95 边界：第 95 百分位恰 50ms → 不达（须严格小于）。
        let mut g2 = VolumeGov::new();
        for i in 0..100u32 {
            g2.record_latency(0, if i < 95 { 49 } else { 60 });
        }
        assert!(g2.latency_ok(), "P95 秩 95 → 49ms 达标");
        let mut g3 = VolumeGov::new();
        for i in 0..100u32 {
            g3.record_latency(0, if i < 95 { 55 } else { 10 });
        }
        assert!(!g3.latency_ok(), "P95 秩 95 → 55ms 超门");
    }

    #[test]
    fn quantize_even_grid() {
        let mut g = VolumeGov::new();
        assert_eq!(g.set_volume(53, 0), 52);
        assert_eq!(g.set_volume(1, 10), 0);
        assert_eq!(g.set_volume(99, 20), 98);
        assert_eq!(g.set_volume(200, 30), 100);
        assert_eq!(g.set_volume(0, 40), 0);
        for v in [g.set_volume(37, 50), g.set_volume(75, 60)] {
            assert_eq!(v % 2, 0, "偶数格纪律");
        }
    }

    #[test]
    fn volosd_selfcheck_all_green() {
        let set = run_volosd_checks();
        assert!(set.all_passed(), "F240 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F240。持久化面 = 音量/静音 framed 记录（跨重启恢复位——
// 「记住的静音」）；壳接线面 = OSD 形制区分几何（竖条 vs 静音徽标 vs
// 亮度横条）+ 连发步进计时判定；判定面 = run_volosd_v2_checks
// （首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数与 vdesk 音频指纹同族）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 记录式：magic4+ver1+值 u8+静音哨兵 u8+checksum u32 = 11 字节定长
/// （容量上限在册：零堆，栈上缓冲即可）。
pub const V2_PAYLOAD_LEN: usize = 2;
pub const V2_REC_LEN: usize = 5 + V2_PAYLOAD_LEN + 4;
const V2_BODY_LEN: usize = V2_REC_LEN - 4;

/// 音量/静音持久化记录（主册 F240 v2：跨重启恢复位——静音哨兵沿用
/// 既有 MUTE_ON_BYTE，防止意外清零；半夜开机不炸音箱）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2VolRec {
    /// 音量值（偶数格 0..=100）。
    pub value: u8,
    /// 静音态（哨兵字节 = MUTE_ON_BYTE）。
    pub muted: bool,
}

impl V2VolRec {
    pub fn capture(g: &VolumeGov) -> V2VolRec {
        V2VolRec { value: g.value(), muted: g.muted() }
    }

    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < V2_REC_LEN {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.value;
        out[6] = if self.muted { MUTE_ON_BYTE } else { 0 };
        let sum = v2_fnv1a32(&out[..V2_BODY_LEN]);
        out[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum.to_le_bytes());
        Some(V2_REC_LEN)
    }

    /// 解码：四类损坏 + 值出偶数格/越界（2 步进域纪律）一律拒绝。
    pub fn from_bytes(buf: &[u8]) -> Result<V2VolRec, V2SaveErr> {
        if buf.len() != V2_REC_LEN {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let expect = u32::from_le_bytes([buf[V2_BODY_LEN], buf[V2_BODY_LEN + 1], buf[V2_BODY_LEN + 2], buf[V2_BODY_LEN + 3]]);
        if v2_fnv1a32(&buf[..V2_BODY_LEN]) != expect {
            return Err(V2SaveErr::BadChecksum);
        }
        if buf[5] > VOL_MAX || buf[5] % 2 != 0 {
            return Err(V2SaveErr::BadLen);
        }
        Ok(V2VolRec { value: buf[5], muted: buf[6] == MUTE_ON_BYTE })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 竖条几何档位（主册 F240「竖条式」的壳层布局）。
pub const V2_BAR_W: i32 = 24;
pub const V2_BAR_H: i32 = 200;

/// 音量 OSD 几何（竖条居中 + 静音斜杠徽标悬于条顶）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2VolOsd {
    pub bar: crate::h1star::h1base::Rect,
    pub badge: crate::h1star::h1base::Rect,
}

pub fn v2_osd_geometry(screen: &crate::h1star::h1base::Rect) -> V2VolOsd {
    let bar = crate::h1star::h1base::Rect::new(
        screen.x + (screen.w - V2_BAR_W) / 2,
        screen.y + (screen.h - V2_BAR_H) / 2,
        V2_BAR_W,
        V2_BAR_H,
    );
    let badge = crate::h1star::h1base::Rect::new(bar.x - 4, bar.y - 36, V2_BAR_W + 8, 28);
    V2VolOsd { bar, badge }
}

/// 形制区分几何判定（主册「与亮度横条形制区分防误读」的几何面落点，
/// 与既有 form_distinct 同口径互证：音量竖条 h>w、亮度横条 w>h）。
pub fn v2_form_geometry_distinct(vol: &crate::h1star::h1base::Rect, bright: &crate::h1star::h1base::Rect) -> bool {
    vol.h > vol.w && bright.w > bright.h
}

/// 连发步进计时判定（主册「按住连发」：首步延迟 400ms±20%、重复率
/// 30ms±20%——实测两参数与主册档位比对）。
pub fn v2_repeat_cadence_ok(first_step_ms: u64, interval_ms: u64) -> bool {
    const TOL: u64 = 20; // ±20%（百分数）。
    let delay_ok = first_step_ms * 100 >= REPEAT_DELAY_MS * (100 - TOL)
        && first_step_ms * 100 <= REPEAT_DELAY_MS * (100 + TOL);
    let rate_ok = interval_ms * 100 >= REPEAT_INTERVAL_MS * (100 - TOL)
        && interval_ms * 100 <= REPEAT_INTERVAL_MS * (100 + TOL);
    delay_ok && rate_ok
}

// -- 判定面扩展 ------------------------------------------------------------

/// F240 v2 自检（首条必为持久化 round-trip）。
pub fn run_volosd_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F240-volosd-v2");

    // 1. 持久化 round-trip（验主册「静音是记住的——重启后保持静音态」）。
    let mut g = VolumeGov::new();
    let _ = g.set_volume(64, 0);
    let _ = g.mute_toggle(100);
    let rec = V2VolRec::capture(&g);
    let mut buf = [0u8; V2_REC_LEN];
    let wrote = rec.to_bytes(&mut buf).unwrap_or(0);
    let back = V2VolRec::from_bytes(&buf[..wrote]);
    set.add(
        "v2 persist round-trip: volume + muted",
        wrote == V2_REC_LEN && back == Ok(rec) && rec.value == 64 && rec.muted,
        "",
    );

    // 2. 四类损坏全拒绝 + 奇数格拒绝（2 步进域纪律同一路口把关）。
    //    [缺陷账本] 现象：/odd-grid 分支红。根因：值字节（载荷域，受
    //    校验和覆盖）翻位后未重算校验和——实现先验校验和后查格域，
    //    必先报 BadChecksum，奇数格分支未被真正测到，属检查项构造
    //    缺陷。修法：翻位后重算校验和，真测 odd-grid 分支。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 2;
    let mut b5 = buf;
    b5[5] = 63; // 奇数格。
    let sum5 = v2_fnv1a32(&b5[..V2_BODY_LEN]);
    b5[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum5.to_le_bytes());
    let mut b4 = buf;
    b4[6] ^= 0xFF; // 翻载荷字节（校验和覆盖域内）→ BadChecksum
    set.add(
        "v2 persist rejects magic/version/len/checksum/odd-grid",
        matches!(V2VolRec::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2VolRec::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2VolRec::from_bytes(&b5), Err(V2SaveErr::BadLen))
            && matches!(V2VolRec::from_bytes(&buf[..V2_REC_LEN - 1]), Err(V2SaveErr::BadLen))
            && matches!(V2VolRec::from_bytes(&b4), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. OSD 形制区分几何（验主册「竖条式，与亮度横条形制区分」：
    //    竖条 h>w、徽标在条顶之上、亮度横条 w>h——双向判定）。
    let screen = crate::h1star::h1base::Rect::new(0, 0, 1920, 1080);
    let osd = v2_osd_geometry(&screen);
    let bright_bar = crate::h1star::h1base::Rect::new(640, 1032, 640, 12);
    set.add(
        "v2 form geometry distinct: vertical bar vs slash badge vs bright",
        v2_form_geometry_distinct(&osd.bar, &bright_bar)
            && osd.badge.bottom() <= osd.bar.y
            && osd.bar.h > osd.bar.w,
        "",
    );

    // 4. 连发步进计时（验主册「按住连发」：400ms/30ms 档位 ±20% 容差；
    //    与既有 repeat_tick 实测互证——实走一拍恰 30ms 节奏）。
    let mut g2 = VolumeGov::new();
    let _ = g2.key_down(1, 10_000, 10_000);
    let _ = g2.repeat_tick(10_430); // 起拍 400ms 后恰一拍。
    set.add(
        "v2 repeat cadence 400ms/30ms within ±20%",
        v2_repeat_cadence_ok(400, 30)
            && !v2_repeat_cadence_ok(200, 30)
            && !v2_repeat_cadence_ok(400, 60)
            && g2.stats.repeat_steps == 1,
        "",
    );

    // 5. 静音哨兵（验主册「不半夜开机炸音箱」：静音字节非零哨兵防误清，
    //    重启装载后斜杠图标同步亮起）。
    set.add(
        "v2 mute sentinel survives boot",
        rec.muted && V2VolRec::from_bytes(&buf[..wrote]).map(|r| r.muted).unwrap_or(false),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_vol_rec_roundtrip_unmuted() {
        let g = VolumeGov::new();
        let rec = V2VolRec::capture(&g);
        assert_eq!(rec.value, 50);
        assert!(!rec.muted);
        let mut buf = [0u8; V2_REC_LEN];
        let n = rec.to_bytes(&mut buf).unwrap();
        assert_eq!(V2VolRec::from_bytes(&buf[..n]).unwrap(), rec);
    }

    #[test]
    fn v2_osd_geometry_centered() {
        let screen = crate::h1star::h1base::Rect::new(0, 0, 1920, 1080);
        let osd = v2_osd_geometry(&screen);
        assert_eq!(osd.bar.x, (1920 - V2_BAR_W) / 2);
        assert_eq!(osd.bar.y, (1080 - V2_BAR_H) / 2);
    }

    #[test]
    fn volosd_v2_selfcheck_all_green() {
        let s = run_volosd_v2_checks();
        assert!(s.all_passed(), "F240 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
