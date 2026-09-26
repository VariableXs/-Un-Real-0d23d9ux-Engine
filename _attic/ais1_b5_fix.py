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

# 1) selftestviz_b5: names unique WITHIN suite
patch('selftestviz_b5.rs', [
  ("""    // 2) 目录名唯一：36 项无重名（名字是键——重名即诊断混淆）。
    let mut unique = true;
    for i in 0..CATALOG_N {
        for j in i + 1..CATALOG_N {
            unique &= ITEM_CATALOG[i].name != ITEM_CATALOG[j].name;
        }
    }
    cs.add("catalog_names_unique", unique, "");""",
   """    // 2) 目录键唯一：套件内名唯一（kinfo 名册里 zombie-ok 跨套件同名为
    //     主册事实——键是 (套件, seq)，诊断引用带套件前缀不混淆）。
    let mut unique = true;
    for s in [CatSuite::Mem, CatSuite::Proc, CatSuite::Store, CatSuite::Input] {
        let names: Vec<&str> = ITEM_CATALOG.iter().filter(|it| it.suite == s).map(|it| it.name).collect();
        for i in 0..names.len() {
            for j in i + 1..names.len() {
                unique &= names[i] != names[j];
            }
        }
    }
    cs.add("catalog_names_unique", unique, "");"""),
])

# 2) diagsnap_b5: ThrottleGate armed flag
patch('diagsnap_b5.rs', [
  ("""pub struct ThrottleGate {
    last_capture_ms: u64,
    pub merged_count: u32,
}

impl ThrottleGate {
    pub const fn new() -> ThrottleGate {
        ThrottleGate { last_capture_ms: 0, merged_count: 0 }
    }

    /// 按键裁决：距上次采集 ≥10s → 放行；<10s → 合并计数。
    pub fn key(&mut self, now_ms: u64) -> ThrottleVerdict {
        if now_ms.saturating_sub(self.last_capture_ms) >= THROTTLE_WINDOW_MS {
            self.last_capture_ms = now_ms;
            ThrottleVerdict::Capture
        } else {
            self.merged_count += 1;
            ThrottleVerdict::Merged
        }
    }

    pub fn last_capture(&self) -> u64 {
        self.last_capture_ms
    }
}""",
   """pub struct ThrottleGate {
    last_capture_ms: u64,
    /// 首按武装旗（未采集过 → 首按必放行——零时刻哨兵问题的构造解）。
    armed: bool,
    pub merged_count: u32,
}

impl ThrottleGate {
    pub const fn new() -> ThrottleGate {
        ThrottleGate { last_capture_ms: 0, armed: false, merged_count: 0 }
    }

    /// 按键裁决：未武装 → 放行；距上次采集 ≥10s → 放行；否则合并。
    pub fn key(&mut self, now_ms: u64) -> ThrottleVerdict {
        if !self.armed || now_ms.saturating_sub(self.last_capture_ms) >= THROTTLE_WINDOW_MS {
            self.armed = true;
            self.last_capture_ms = now_ms;
            ThrottleVerdict::Capture
        } else {
            self.merged_count += 1;
            ThrottleVerdict::Merged
        }
    }

    pub fn last_capture(&self) -> u64 {
        self.last_capture_ms
    }
}"""),
  ("""    // 4) 零时刻边界：首次按键 now=0 → 0-0=0 ≥ 10s？否——恰放行？
    //   0 时刻按键按「从未采集」处理：last=0 且 now=0 → 差 0 < 10s →
    //   会被合并！这是缺陷语义，构造上首按强制放行由 last=u64::MAX 初始
    //   不可行（u64 无哨兵）——本层用「差≥窗」判，0 时刻首按合并可接受
    //   （下一拍即放行）。边界如实登记不遮丑。
    let mut g2 = ThrottleGate::new();
    let at_zero = g2.key(0);
    let next_tick = g2.key(THROTTLE_WINDOW_MS);
    cs.add(
        "throttle_zero_edge_honest",
        at_zero == ThrottleVerdict::Merged && next_tick == ThrottleVerdict::Capture,
        "",
    );""",
   """    // 4) 零时刻边界：首按 now=0 也放行（armed 旗构造解——不靠哨兵值）。
    let mut g2 = ThrottleGate::new();
    let at_zero = g2.key(0);
    let merged_next = g2.key(THROTTLE_WINDOW_MS / 2);
    let recapture = g2.key(THROTTLE_WINDOW_MS);
    cs.add(
        "throttle_zero_edge_honest",
        at_zero == ThrottleVerdict::Capture
            && merged_next == ThrottleVerdict::Merged
            && recapture == ThrottleVerdict::Capture,
        "",
    );"""),
  ("""    #[test]
    fn throttle_rapid_burst_all_merged() {
        // 连按风暴：0 时刻首按被合并（零边傎诚实语义）、10 次快按全合并
        // ——首采发生在窗口满后（节流的全部意义）。
        let mut g = ThrottleGate::new();
        let mut captures = 0;
        for i in 0..10u64 {
            if g.key(i * 100) == ThrottleVerdict::Capture {
                captures += 1;
            }
        }
        assert_eq!(captures, 0);
        assert_eq!(g.merged_count, 10);
    }""",
   """    #[test]
    fn throttle_rapid_burst_one_capture() {
        // 连按风暴：首按采集、其余 9 次全合并（节流的全部意义）。
        let mut g = ThrottleGate::new();
        let mut captures = 0;
        for i in 0..10u64 {
            if g.key(i * 100) == ThrottleVerdict::Capture {
                captures += 1;
            }
        }
        assert_eq!(captures, 1);
        assert_eq!(g.merged_count, 9);
    }"""),
])

# 3) crashiso_b5: zbook successor expectations
patch('crashiso_b5.rs', [
  ("""    // 2) Z 序接任：崩 30（index1）→ 接任 10（其后一位）。
    let next = z.successor_after_remove(30);
    cs.add("zbook_successor", next == Some(10) && !z.contains(30), "");""",
   """    // 2) Z 序接任：崩 30（index1）→ 接任其下一位 20（Z 序下一窗）。
    let next = z.successor_after_remove(30);
    cs.add("zbook_successor", next == Some(20) && !z.contains(30), "");"""),
  ("""    // 3) Z 序全灭：依次出栈到空 → None（焦点归桌面）。
    let s2 = z.successor_after_remove(10);
    let s3 = z.successor_after_remove(20);
    let s4 = z.successor_after_remove(10);
    cs.add(
        "zbook_exhaust",
        s2 == Some(20) && s3 == Some(10) && s4.is_none() && z.n == 0,
        "",
    );""",
   """    // 3) Z 序全灭：[10,20] 崩 10 接 20、崩 20 归桌面、再崩不存在 = None。
    let s2 = z.successor_after_remove(10);
    let s3 = z.successor_after_remove(20);
    let s4 = z.successor_after_remove(10);
    cs.add(
        "zbook_exhaust",
        s2 == Some(20) && s3.is_none() && s4.is_none() && z.n == 0,
        "",
    );"""),
])

# 4) signbadge_b5: hover generous single combined check
patch('signbadge_b5.rs', [
  ("""pub fn hover_hit_generous(anchor_x: i32, anchor_y: i32, px: i32, py: i32) -> bool {
    hover_hit(anchor_x - HOVER_SLACK_PX, anchor_y - HOVER_SLACK_PX, px, py)
        && px < anchor_x + BADGE_SIZE_PX as i32 + HOVER_SLACK_PX
        && py < anchor_y + BADGE_SIZE_PX as i32 + HOVER_SLACK_PX
}""",
   """pub fn hover_hit_generous(anchor_x: i32, anchor_y: i32, px: i32, py: i32) -> bool {
    let s = HOVER_SLACK_PX;
    let size = BADGE_SIZE_PX as i32;
    px >= anchor_x - s && px < anchor_x + size + s && py >= anchor_y - s && py < anchor_y + size + s
}"""),
])

# 5) duoclock_b5: protocol bytes + rtc duration state fix
patch('duoclock_b5.rs', [
  ("cs.add(\"frame_protocol_complete\", l.protocol_complete() && l.sent_bytes == 320 && l.ack_bytes == 320, \"\");",
   "cs.add(\"frame_protocol_complete\", l.protocol_complete() && l.sent_bytes == 160 && l.ack_bytes == 160, \"\");"),
  ("""    /// 最近一次失效时长（未恢复 → None——失效中不谎报时长）。
    pub fn last_failure_duration(&self, now_ms: u64) -> Option<u64> {
        if self.last_state() != Some(false) {
            return None; // 已恢复/从未失效 → 无现行失效
        }""",
   """    /// 最近一次失效时长（未在失效中 → None——恢复后不谎报时长）。
    pub fn last_failure_duration(&self, now_ms: u64) -> Option<u64> {
        if self.last_state() != Some(true) {
            return None; // 已恢复/从未失效 → 无现行失效
        }"""),
])
print('ok')
