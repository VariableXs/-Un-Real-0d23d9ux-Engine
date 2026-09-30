//! F026 深化批次三 · DirectSound 缓冲执行/边界/注入面（compatstar2/deep2 · G-A-26）。
//!
//! 批次一/二深化覆盖 WinMM 时序与混音主干；本批补齐主册【功能定义】「全语义
//! 对齐」的 DirectSound 缓冲侧出口：环形缓冲双游标推进模型（play/write 定长
//! 4096 环、回绕取模、欠载检出计数）、DSBPOSITIONNOTIFY 通知位置表（定长 8
//! 多点触发——游标越过即触发且触发后按标记位去重）、dB 音量定点衰减表
//! （DSBVOLUME_MIN=-10000 满量程、6dB 减半的定点近似）、3D 声像三角衰减模型
//! （左/右衰减系数按 pan 归一化，±10000 满幅）、缓冲丢失→恢复四态状态机
//! （lost→acquire→restore→playing，非法转换显性拒绝）。
//!
//! 判据对账：深化以主册【设计细节】/【状态与异常】未落地面为源，一处一事实
//! （MS IDirectSoundBuffer::SetVolume/SetPan/DSBPOSITIONNOTIFY 文档语义对拍）。
//!
//! 零堆纪律：定长环 + 定长通知表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 环形缓冲定长 4096（DirectSound 次缓冲定长口径；游标 DWORD 回绕取模）。
pub const RING_SIZE: u32 = 4096;
/// DSBVOLUME_MIN = -10000（MS：-100dB 满量程衰减；DSBVOLUME_MAX = 0）。
pub const DSBVOLUME_MIN: i32 = -10000;
/// DSBVOLUME_MAX = 0（MS：0dB 不衰减即满幅）。
pub const DSBVOLUME_MAX: i32 = 0;
/// DSBPAN 满幅（MS SetPan：-10000 左满幅 ~ +10000 右满幅）。
pub const DSBPAN_FULL: i32 = 10000;
/// 6dB 减半近似步长（百分 dB 口径：-600 ≈ -6dB → 振幅右移一位）。
pub const HALF_STEP_HUNDREDTHS_DB: i32 = 600;
/// DSBPOSITIONNOTIFY 通知位置表容量（DSBPOSITIONNOTIFY 数组，域内口径）。
pub const MAX_NOTIFY_POINTS: usize = 8;
/// Q8.8 定点满幅振幅（256 = 1.0）。
pub const AMP_UNITY_Q8: u32 = 256;
/// 定点右移上限（-96dB 及以下按静音处理——256>>16=0 饱和面）。
pub const AMP_SHIFT_CAP: u32 = 16;

// ---------------------------------------------------------------------------
// 环形缓冲双游标模型
// ---------------------------------------------------------------------------

/// 环形缓冲双游标（自由计数 + 取模下标——DirectSound 游标 DWORD 语义）。
pub struct DsRing {
    /// 播放游标（自由计数）。
    pub play_cursor: u32,
    /// 写入游标（自由计数，恒 >= play）。
    pub write_cursor: u32,
    /// 欠载检出计数（play 越过 write 请求——不静默）。
    pub underruns: u32,
    /// 满环写入拒绝计数（不静默覆盖）。
    pub write_rejects: u32,
}

impl DsRing {
    pub const fn new() -> Self { DsRing { play_cursor: 0, write_cursor: 0, underruns: 0, write_rejects: 0 } }

    /// 游标 → 环内下标（回绕取模）。
    pub fn index_of(cursor: u32) -> usize {
        (cursor % RING_SIZE) as usize
    }

    /// 可读量（write - play；自由计数差恒不回绕负值）。
    pub fn avail(&self) -> u32 {
        self.write_cursor.wrapping_sub(self.play_cursor)
    }

    /// 生产侧推进：返回实际接收字节数；超容量部分显性拒绝计数。
    pub fn produce(&mut self, want: u32) -> u32 {
        let free = RING_SIZE - self.avail();
        if want > free {
            self.write_rejects += 1;
        }
        let got = want.min(free);
        self.write_cursor = self.write_cursor.wrapping_add(got);
        got
    }

    /// 消费侧推进：请求越过可读量 → 欠载检出计数，步长夹回 avail。
    pub fn advance_play(&mut self, want: u32) -> u32 {
        let avail = self.avail();
        if want > avail {
            self.underruns += 1;
        }
        let step = want.min(avail);
        self.play_cursor = self.play_cursor.wrapping_add(step);
        step
    }
}

// ---------------------------------------------------------------------------
// DSBPOSITIONNOTIFY 通知位置表
// ---------------------------------------------------------------------------

/// 一个通知点（MS DSBPOSITIONNOTIFY：dwOffset + hEventNotify 对的偏移半边）。
#[derive(Clone, Copy)]
pub struct NotifyPoint { pub offset: u32, pub fired: bool }

/// 通知位置表（定长 8，多点触发）。
pub struct NotifyTable {
    pub points: [NotifyPoint; MAX_NOTIFY_POINTS],
    pub len: usize,
    /// 表满登记拒绝计数（不静默覆盖）。
    pub overflow_rejects: u32,
    /// 累计触发次数（账面）。
    pub trigger_count: u32,
}

impl NotifyTable {
    pub const fn new() -> Self {
        NotifyTable {
            points: [NotifyPoint { offset: 0, fired: false }; MAX_NOTIFY_POINTS],
            len: 0, overflow_rejects: 0, trigger_count: 0,
        }
    }

    /// 登记通知点；表满 → 显性拒绝并计数。
    pub fn add(&mut self, offset: u32) -> Result<(), &'static str> {
        if self.len >= MAX_NOTIFY_POINTS {
            self.overflow_rejects += 1;
            return Err("notify-table-full");
        }
        self.points[self.len] = NotifyPoint { offset: offset % RING_SIZE, fired: false };
        self.len += 1;
        Ok(())
    }

    /// 越过判定：点位 ∈ (old, new]；回绕（new < old）补 (old, RING) ∪ [0, new]。
    fn crossed(offset: u32, old: u32, new: u32) -> bool {
        if new >= old {
            offset > old && offset <= new
        } else {
            offset > old || offset <= new
        }
    }

    /// 游标推进后扫表：越过即触发并置标记位去重；返回本次触发数。
    pub fn poll(&mut self, old_ring: u32, new_ring: u32) -> u32 {
        let mut fired = 0;
        for i in 0..self.len {
            if !self.points[i].fired && Self::crossed(self.points[i].offset, old_ring, new_ring) {
                self.points[i].fired = true;
                self.trigger_count += 1;
                fired += 1;
            }
        }
        fired
    }

    /// 重新布防（应用更新 SetNotificationPositions 后复用）。
    pub fn rearm(&mut self) {
        for i in 0..self.len {
            self.points[i].fired = false;
        }
    }
}

// ---------------------------------------------------------------------------
// dB 音量 / 3D 声像衰减
// ---------------------------------------------------------------------------

/// dB→振幅定点衰减（Q8.8；6dB 减半整数近似：每 -600 百分 dB 右移一位）。
/// 出处：MS SetVolume 语义（DSBVOLUME_MIN=-10000 满量程）+ 主册 G-A-26 音量判据。
pub fn amp_q8(volume_hundredths_db: i32) -> u32 {
    if volume_hundredths_db >= DSBVOLUME_MAX {
        return AMP_UNITY_Q8;
    }
    let atten = (-volume_hundredths_db).min(-DSBVOLUME_MIN) as u32; // 0..=10000
    let shifts = atten / HALF_STEP_HUNDREDTHS_DB as u32;
    AMP_UNITY_Q8 >> shifts.min(AMP_SHIFT_CAP)
}

/// 3D 声像三角衰减：线性归一化，±10000 满幅时对侧归零。
/// 出处：MS SetPan 语义 + 主册 G-A-26（pan 判据）。
pub fn pan_gains_q8(pan: i32) -> (u32, u32) {
    let p = pan.clamp(-DSBPAN_FULL, DSBPAN_FULL);
    if p >= 0 {
        let left = AMP_UNITY_Q8 * (DSBPAN_FULL - p) as u32 / DSBPAN_FULL as u32;
        (left, AMP_UNITY_Q8)
    } else {
        let right = AMP_UNITY_Q8 * (DSBPAN_FULL + p) as u32 / DSBPAN_FULL as u32;
        (AMP_UNITY_Q8, right)
    }
}

// ---------------------------------------------------------------------------
// 缓冲丢失→恢复状态机
// ---------------------------------------------------------------------------

/// 缓冲四态（lost→acquire→restore→playing；playing 失焦/独占抢占回 lost）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufState { Lost, Acquired, Restored, Playing }

/// 缓冲丢失→恢复状态机（非法转换显性拒绝并计数——主册【状态与异常】）。
pub struct BufFsm {
    pub state: BufState,
    /// 非法转换检出计数。
    pub illegal_transitions: u32,
}

impl BufFsm {
    pub const fn new() -> Self { BufFsm { state: BufState::Lost, illegal_transitions: 0 } }

    /// 单步转换；合法序表外一律 Err 并计数（零静默吞错）。
    pub fn transition(&mut self, to: BufState) -> Result<(), &'static str> {
        let legal = matches!(
            (self.state, to),
            (BufState::Lost, BufState::Acquired)
                | (BufState::Acquired, BufState::Restored)
                | (BufState::Restored, BufState::Playing)
                | (BufState::Playing, BufState::Lost)
        );
        if legal {
            self.state = to;
            Ok(())
        } else {
            self.illegal_transitions += 1;
            Err("illegal-transition")
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次三）
// ---------------------------------------------------------------------------

pub fn run_f026e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F026-dsound-buf-d3");
    // 1) 双游标回绕取模：满环接收 4096，写游标 4096 → 环内下标回绕 0。
    let mut r = DsRing::new();
    let got = r.produce(5000);
    cs.add(
        "dsring_cursor_wrap",
        got == RING_SIZE && r.write_cursor == 4096 && DsRing::index_of(r.write_cursor) == 0, "",
    );
    // 2) 欠载检出：avail=4096 请求推进 5000 → 计 1 次欠载，步长夹回 4096。
    let step = r.advance_play(5000);
    cs.add("dsring_underrun_count", step == RING_SIZE && r.underruns == 1 && r.play_cursor == 4096, "");
    // 3) 满环写入拒绝：再整环填满后第 11 字节显性拒绝计数（写游标不动）。
    let filled = r.produce(RING_SIZE);
    let got2 = r.produce(10);
    cs.add(
        "dsring_full_write_reject",
        filled == RING_SIZE && got2 == 0 && r.write_cursor == 8192 && r.write_rejects == 2,
        "",
    );
    // 4) 通知多点触发：整环扫过 1024/3072 两点均触发（触发后标记位去重）。
    let mut n = NotifyTable::new();
    let _ = n.add(1024);
    let _ = n.add(3072);
    let fired = n.poll(0, 4096);
    cs.add("notify_multipoint_fire", fired == 2 && n.trigger_count == 2, "");
    // 5) 标记位去重：同窗口二次扫表零触发。
    cs.add("notify_dedupe", n.poll(0, 4096) == 0 && n.trigger_count == 2, "");
    // 6) 重新布防后复触发 + 回绕窗口（4090→30 越过 4095 与 10 两点）。
    n.rearm();
    let fired_rearm = n.poll(0, 4096);
    let mut n2 = NotifyTable::new();
    let _ = n2.add(10);
    let _ = n2.add(4095);
    let fired_wrap = n2.poll(4090, 30);
    cs.add(
        "notify_rearm_wrap_window",
        fired_rearm == 2 && n.trigger_count == 4 && fired_wrap == 2 && n2.trigger_count == 2,
        "",
    );
    // 7) 6dB 减半定点近似：0dB 满幅、-6dB 减半、-18dB 八分之一。
    cs.add(
        "volume_6db_halving",
        amp_q8(0) == 256 && amp_q8(-600) == 128 && amp_q8(-1800) == 32,
        "",
    );
    // 8) DSBVOLUME_MIN=-10000 满量程 → 静音；正值夹回满幅。
    cs.add("volume_min_silence", amp_q8(DSBVOLUME_MIN) == 0 && amp_q8(500) == 256, "");
    // 9) 声像三角衰减：满右对侧归零、满左对称、居中双满幅、半程 128/256。
    cs.add(
        "pan_triangle_gains",
        pan_gains_q8(10000) == (0, 256)
            && pan_gains_q8(-10000) == (256, 0)
            && pan_gains_q8(0) == (256, 256)
            && pan_gains_q8(5000) == (128, 256),
        "",
    );
    // 10) 状态机合法全循环：lost→acquire→restore→playing→lost。
    let mut f = BufFsm::new();
    let legal_path = f.transition(BufState::Acquired).is_ok()
        && f.transition(BufState::Restored).is_ok()
        && f.transition(BufState::Playing).is_ok()
        && f.state == BufState::Playing
        && f.transition(BufState::Lost).is_ok()
        && f.state == BufState::Lost;
    cs.add("buf_fsm_legal_cycle", legal_path, "");
    // 11) 非法转换显性拒绝：Lost 态跳步与跳双步均 Err 并计数。
    let skip = f.transition(BufState::Playing);
    let skip2 = f.transition(BufState::Restored);
    cs.add(
        "buf_fsm_illegal_rejected",
        skip == Err("illegal-transition")
            && skip2 == Err("illegal-transition")
            && f.illegal_transitions == 2
            && f.state == BufState::Lost,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_over_empty_ring_counts_underrun() {
        let mut r = DsRing::new();
        let step = r.advance_play(100);
        assert_eq!(step, 0, "空环无可读量，步长夹回 0");
        assert_eq!(r.underruns, 1, "欠载如实计数不静默");
        assert_eq!(r.play_cursor, 0);
    }

    #[test]
    fn notify_table_full_rejects() {
        let mut n = NotifyTable::new();
        for i in 0..MAX_NOTIFY_POINTS {
            assert!(n.add(i as u32 * 64).is_ok(), "容量内登记必成");
        }
        assert_eq!(n.add(4095), Err("notify-table-full"));
        assert_eq!(n.overflow_rejects, 1, "表满拒绝显性计数");
        assert_eq!(n.len, MAX_NOTIFY_POINTS);
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f026e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
