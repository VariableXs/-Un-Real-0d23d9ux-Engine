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

# capenforce_b3: newly created slot must notify immediately
patch('capenforce_b3.rs', [
  ("""        let i = match slot {
            Some(i) => i,
            None => {
                // 新应用开槽（满容复用 0 槽——通知面丢槽优于 panic）。
                let i = self.app_ids.iter().position(|a| a.is_none()).unwrap_or(0);
                self.app_ids[i] = Some(app_id);
                i
            }
        };
        // 槽存在即按冷却窗判（last=0 是合法时刻不是哨兵——t=0 通知也算数）。
        let last = self.last_notify[i];
        if now_ms.saturating_sub(last) >= STORM_WINDOW_MS {
            self.last_notify[i] = now_ms;
            true
        } else {
            false
        }""",
   """        let i = match slot {
            Some(i) => i,
            None => {
                // 新应用开槽（满容复用 0 槽——通知面丢槽优于 panic）。
                let i = self.app_ids.iter().position(|a| a.is_none()).unwrap_or(0);
                self.app_ids[i] = Some(app_id);
                self.last_notify[i] = now_ms;
                return true; // 首次拦截必通知——新应用不该被旧冷却冤枉
            }
        };
        // 老槽按冷却窗判（last=0 是合法时刻不是哨兵——t=0 通知也算数）。
        let last = self.last_notify[i];
        if now_ms.saturating_sub(last) >= STORM_WINDOW_MS {
            self.last_notify[i] = now_ms;
            true
        } else {
            false
        }"""),
])

# signbadge_b3: center the 8-bit shape in the 12px grid (2px margins)
patch('signbadge_b3.rs', [
  ("""    for (row, bits) in shape.iter().enumerate() {
        for col in 0..8.min(GRID) {
            if bits & (0x80 >> col) != 0 {
                out[row * GRID + col] = color;
            }
        }
    }""",
   """    // 8bit 形状在 12px 网格中水平居中（左右各 2px 边距——对称轴即网格轴）。
    let x_off = (GRID - 8) / 2;
    for (row, bits) in shape.iter().enumerate() {
        for s in 0..8 {
            if bits & (0x80 >> s) != 0 {
                out[row * GRID + x_off + s] = color;
            }
        }
    }"""),
])

print("patched")
