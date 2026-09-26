# -*- coding: utf-8 -*-
import os
BASE = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\secstar"

def patch(p, subs):
    fp = os.path.join(BASE, p)
    s = open(fp, encoding='utf-8').read()
    for old, new in subs:
        assert old in s, (p, old[:70])
        s = s.replace(old, new, 1)
    open(fp, 'w', encoding='utf-8').write(s)

# bootmenu_b3: 7 nexts reach page-0 end (page_len=8)
patch('bootmenu_b3.rs', [
  ("""    let mut p2 = Paginator::new(20);
    for _ in 0..8 {
        p2.next();
    }""",
   """    let mut p2 = Paginator::new(20);
    for _ in 0..7 {
        p2.next();
    }"""),
])

# panicscreen_b3: texts must actually exceed one line
patch('panicscreen_b3.rs', [
  ("""    let text = *b"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron";""",
   """    let text = *b"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon";"""),
  ("""    let phrase: &[u8] = "内核遇到无法恢复的错误需要重启请保存工作内容并查看帮助篇以了解更多信息与修复步骤".as_bytes();""",
   """    let phrase: &[u8] = "内核遇到无法恢复的错误需要重启请保存工作内容并查看帮助篇以了解更多信息与修复步骤请耐心等待系统完成保存".as_bytes();"""),
])

# crashiso_b3: quarantined slots cannot crash/restart (frozen awaiting human)
patch('crashiso_b3.rs', [
  ("""    /// 记一次崩溃：计数+1、状态转 Crashed（返回是否仍在重启配额内）。
    pub fn on_crash(&mut self, app_id: u32) -> bool {
        match self.slot_of(app_id) {
            Some(i) => {
                let sl = self.slots[i].as_mut().unwrap();
                sl.crash_count += 1;""",
   """    /// 记一次崩溃：计数+1、状态转 Crashed（返回是否仍在重启配额内）。
    /// 隔离态应用不参与（冻结等待人工——隔离的意义就在这里）。
    pub fn on_crash(&mut self, app_id: u32) -> bool {
        match self.slot_of(app_id) {
            Some(i) if self.slots[i].as_ref().unwrap().state == SlotState::Quarantined => false,
            Some(i) => {
                let sl = self.slots[i].as_mut().unwrap();
                sl.crash_count += 1;"""),
])

# duoclock_b3: slew step direction follows offset sign (positive offset -> +100 step subtracted)
patch('duoclock_b3.rs', [
  ("""        decide_correction(0) == Correction::None
            && decide_correction(300) == Correction::Slew(-100)
            && decide_correction(-300) == Correction::Slew(100)
            && decide_correction(2_000) == Correction::Step,""",
   """        decide_correction(0) == Correction::None
            && decide_correction(300) == Correction::Slew(100)
            && decide_correction(-300) == Correction::Slew(-100)
            && decide_correction(2_000) == Correction::Step,"""),
])

# diskhealth_b3: band feeds USED permille into main scale; edge check aligned
patch('diskhealth_b3.rs', [
  ("""    /// 剩余寿命落段（复用主层三段阈值——同一把尺子量两件事不另造线）。
    pub fn band(&self, written_bytes: u64) -> Band {
        band_of(self.remaining_permille(written_bytes))
    }""",
   """    /// 剩余寿命落段（复用主层三段阈值——主层尺量的是**已用**占比，
    /// 这里把剩余换算成已用再过尺，同一把尺不另造线）。
    pub fn band(&self, written_bytes: u64) -> Band {
        band_of(1_000 - self.remaining_permille(written_bytes))
    }"""),
  ("""    // 13) 主层尺贯通：band_of 边界值（600/850 千分线）与主层同源。
    cs.add(
        "band_edges_same_scale",
        matches!(band_of(0), Band::Red) && matches!(band_of(650), Band::Yellow) && matches!(band_of(999), Band::Green),
        "",
    );""",
   """    // 13) 主层尺贯通：band_of 边界值（主册口径——已用 <60% 绿 / <85%
    // 黄 / ≥85% 红）与主层同源（主层 band_of 逐点对拍）。
    cs.add(
        "band_edges_same_scale",
        matches!(band_of(0), Band::Green)
            && matches!(band_of(599), Band::Green)
            && matches!(band_of(600), Band::Yellow)
            && matches!(band_of(849), Band::Yellow)
            && matches!(band_of(850), Band::Red)
            && matches!(band_of(999), Band::Red),
        "",
    );"""),
])

# hotplug_b3: flushed 60 of 100 -> 600 permille
patch('hotplug_b3.rs', [
  ("""    let mid = !fp.complete() && fp.permille(100) == 400;""",
   """    let mid = !fp.complete() && fp.permille(100) == 600;"""),
])

# romount_b3: retry loop needs a copying phase before fail()
patch('romount_b3.rs', [
  ("""    let mut w6 = CopyWorker::new(1_000);
    let mut retries_ok = 0;
    for _ in 0..4 {
        w6.fail();
        if w6.phase == CopyPhase::Failed && w6.retry() {
            retries_ok += 1;
        }
    }""",
   """    let mut w6 = CopyWorker::new(1_000);
    let mut retries_ok = 0;
    for _ in 0..4 {
        w6.start_scan();
        w6.start_copy();
        w6.fail();
        if w6.phase == CopyPhase::Failed && w6.retry() {
            retries_ok += 1;
        }
    }"""),
])

print("patched round 3")
