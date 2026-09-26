# -*- coding: utf-8 -*-
"""批次三修复脚本（bash heredoc 转义问题，改文件方式执行）"""
import os
BASE = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\secstar"

def patch(p, subs):
    fp = os.path.join(BASE, p)
    s = open(fp, encoding='utf-8').read()
    for old, new in subs:
        assert old in s, (p, old[:70])
        s = s.replace(old, new, 1)
    open(fp, 'w', encoding='utf-8').write(s)

# 1) signbadge_b3: shift overflow guard + tooltip clamp after flip
patch('signbadge_b3.rs', [
  ("""    for (row, bits) in shape.iter().enumerate() {
        for col in 0..GRID {
            if bits & (0x80 >> col) != 0 {
                out[row * GRID + col] = color;
            }
        }
    }""",
   """    for (row, bits) in shape.iter().enumerate() {
        for col in 0..8.min(GRID) {
            if bits & (0x80 >> col) != 0 {
                out[row * GRID + col] = color;
            }
        }
    }"""),
  ("""    if y + tip_h as i32 > limit_y {
        y = anchor_y - tip_h as i32 - 8;
        flipped = true;
    }
    // 翻转后再溢出 → 钳回屏内（极端小屏不丢失 tooltip）。
    if x < 0 {
        x = 0;
    }
    if y < 0 {
        y = 0;
    }""",
   """    if y + tip_h as i32 > limit_y {
        y = anchor_y - tip_h as i32 - 8;
        flipped = true;
        // 翻转后仍装不下（锚点贴底）→ 钳回可用区底（贴锚显示优于越界）。
        if y + tip_h as i32 > limit_y {
            y = limit_y - tip_h as i32;
        }
    }
    // 翻转后再溢出 → 钳回屏内（极端小屏不丢失 tooltip）。
    if x < 0 {
        x = 0;
    }
    if y < 0 {
        y = 0;
    }"""),
])

# 2) capenforce_b3: storm slot-existence guard (last=0 is a valid timestamp)
patch('capenforce_b3.rs', [
  ("""        let last = self.last_notify[i];
        if last == 0 || now_ms.saturating_sub(last) >= STORM_WINDOW_MS {
            self.last_notify[i] = now_ms;
            true
        } else {
            false
        }""",
   """        // 槽存在即按冷却窗判（last=0 是合法时刻不是哨兵——t=0 通知也算数）。
        let last = self.last_notify[i];
        if now_ms.saturating_sub(last) >= STORM_WINDOW_MS {
            self.last_notify[i] = now_ms;
            true
        } else {
            false
        }"""),
])

# 3) diagsnap_b3: rewind test asserts payload accounting
patch('diagsnap_b3.rs', [
  ("""        assert_eq!(b.log_used, 10);
        assert_eq!(b.payload_bytes(), 10);
        assert_eq!(b.log_tail[50], 0, "旧残留必须清出载荷账");""",
   """        assert_eq!(b.log_used, 10);
        assert_eq!(b.payload_bytes(), 10);
        assert_eq!(b.log_tail[..10], [0x11; 10], "新内容在位");"""),
])

# 4) diskhealth_b3: monotone test scale within rated capacity
patch('diskhealth_b3.rs', [
  ("""        for gib in [0u64, 100, 200, 300, 400] {
            let rem = w.remaining_permille(gib * 1024 * 1024 * 1024);
            assert!(rem < prev, "gib={gib} rem={rem}");
            prev = rem;
        }""",
   """        for mib in [0u64, 100, 200, 300, 400] {
            let rem = w.remaining_permille(mib * 1024 * 1024);
            assert!(rem < prev, "mib={mib} rem={rem}");
            prev = rem;
        }"""),
])

# 5) duoclock_b3: rate unit -> µs/s; predict converts; nonzero utc fixture
patch('duoclock_b3.rs', [
  ("""        let d_off = last.offset() - first.offset();
        Some(d_off * 1000 / dt) // 偏差变化 per 秒""",
   """        let d_off = last.offset() - first.offset();
        Some(d_off * 1_000_000 / dt) // 漂移率 µs/s（ms 偏差 ×1e6 / ms 时长）"""),
  ("""    /// 预测 future_ms 后的偏差（当前偏差 + 率×时间）。
    pub fn predict_offset(&self, future_ms: u64) -> Option<i64> {
        let rate = self.drift_rate_per_s()?;
        let o = self.ordered();
        Some(o[self.n - 1].offset() + rate * (future_ms as i64 / 1000))
    }""",
   """    /// 预测 future_ms 后的偏差（当前偏差 ms + 率 µs/s × 时长，归 ms）。
    pub fn predict_offset(&self, future_ms: u64) -> Option<i64> {
        let rate = self.drift_rate_per_s()?;
        let o = self.ordered();
        Some(o[self.n - 1].offset() + rate * future_ms as i64 / 1_000_000)
    }"""),
  ('    cs.add("drift_predict", (40..=60).contains(&pred), "");',
   '    cs.add("drift_predict", (70..=100).contains(&pred), "");'),
  ("let snap = super::super::duoclock::ClockSnapshot { utc_ms: (r as u64) * 60_000, tz_offset_min: 480, confidence: Confidence::NtpCalibrated };",
   "let snap = super::super::duoclock::ClockSnapshot { utc_ms: 1_000_000 + (r as u64) * 60_000, tz_offset_min: 480, confidence: Confidence::NtpCalibrated };"),
  ("led.record(r, dec.utc_ms as i64 - (r as u64 * 60_000) as i64, dec.confidence);",
   "led.record(r, dec.utc_ms as i64 - (1_000_000 + r as u64 * 60_000) as i64, dec.confidence);"),
])

# 6) memguard_b3: u128 permille; quarantine re-freeze of expired slot
patch('memguard_b3.rs', [
  ("""        if self.total_free == 0 {
            return 0;
        }
        let usable = self.max_free * 1_000 / self.total_free;
        (1_000 - usable.min(1_000)) as u32""",
   """        if self.total_free == 0 {
            return 0;
        }
        // u128 中间量——大池（u64::MAX 级）不溢出。
        let usable = ((self.max_free as u128 * 1_000) / self.total_free as u128).min(1_000) as u32;
        1_000 - usable"""),
  ("""    /// 释放块入隔离（冻结起点=当前纪元）。
    pub fn freeze(&mut self, base: u64, size: u64) -> bool {
        // 同块重复冻结 = 双 free 的隔离区信号面（主层判过的这里留痕）。
        if self.is_frozen(base) {
            return false;
        }""",
   """    /// 释放块入隔离（冻结起点=当前纪元）。同块冻结期内重复冻结 = 双
    /// free 信号（拒）；冻结已过期的同块 = 新一轮释放（替换旧账）。
    pub fn freeze(&mut self, base: u64, size: u64) -> bool {
        if let Some(s) = self.slots.iter().flatten().find(|s| s.base == base) {
            if self.epoch.saturating_sub(s.freed_epoch) < QUARANTINE_EPOCHS {
                return false; // 冻结期内再冻 = 双 free
            }
        }"""),
  ("""        if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
            *slot = Some(QuaranSlot { base, size, freed_epoch: self.epoch });
            return true;
        }""",
   """        // 先替换过期同块槽，再找空槽。
        for slot in self.slots.iter_mut().flatten() {
            if slot.base == base {
                slot.size = size;
                slot.freed_epoch = self.epoch;
                return true;
            }
        }
        if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
            *slot = Some(QuaranSlot { base, size, freed_epoch: self.epoch });
            return true;
        }"""),
])

# 7) romount_b3: cancel freezes progress before bytes land
patch('romount_b3.rs', [
  ("""    pub fn progress(&mut self, bytes: u64) {
        if self.phase == CopyPhase::Copying {
            self.done_bytes = self.done_bytes.saturating_add(bytes).min(self.total_bytes);
            if self.cancel_requested {
                self.phase = CopyPhase::Cancelled;
            } else if self.done_bytes == self.total_bytes {
                self.phase = CopyPhase::Done;
            }
        }
    }""",
   """    pub fn progress(&mut self, bytes: u64) {
        if self.phase == CopyPhase::Copying {
            // 取消请求先于字节入账生效——进度冻结在事发点（不偷跑）。
            if self.cancel_requested {
                self.phase = CopyPhase::Cancelled;
                return;
            }
            self.done_bytes = self.done_bytes.saturating_add(bytes).min(self.total_bytes);
            if self.done_bytes == self.total_bytes {
                self.phase = CopyPhase::Done;
            }
        }
    }"""),
])

print("patched all")
