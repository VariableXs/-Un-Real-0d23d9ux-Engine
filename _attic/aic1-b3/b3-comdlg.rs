
// ---------------------------------------------------------------------------
// F008 · 深化批次三：颜色自定义色板持久化（校验和序列化）+ 对话框键盘状态机
// （Enter 确定 / Esc 取消 / Tab 焦点循环）
//
// 主册依据（G-A-08【数据与存储】）：「颜色自定义色板持久化」；【交互设计】
// 「键盘全可达，Enter 确定 Esc 取消」。CUSTOM_COLOR_SLOTS/DialogKind 既有面
// 不重复。
// ---------------------------------------------------------------------------

/// 色板序列化魔数（VXCP——Varix Custom Palette）。
pub const PALETTE_MAGIC: [u8; 4] = *b"VXCP";
/// 序列化尺寸：魔数 4 + 槽位 16×4 + 校验和 4。
pub const PALETTE_SERIAL_SIZE: usize = 4 + CUSTOM_COLOR_SLOTS * 4 + 4;
/// 空槽位哨兵（0xFFFFFFFF——合法 RGB 不会撞上）。
pub const PALETTE_EMPTY_SLOT: u32 = 0xFFFF_FFFF;

/// 自定义色板（16 槽——CHOOSECOLOR 自定义色持久化模型）。
#[derive(Clone, Copy, Debug)]
pub struct CustomPalette {
    slots: [Option<u32>; CUSTOM_COLOR_SLOTS],
    used: usize,
}

impl CustomPalette {
    pub const fn new() -> CustomPalette {
        CustomPalette { slots: [None; CUSTOM_COLOR_SLOTS], used: 0 }
    }

    /// 写槽位（重复写同槽不重复计数）。
    pub fn set(&mut self, slot: usize, rgb: u32) -> bool {
        if slot >= CUSTOM_COLOR_SLOTS || rgb == PALETTE_EMPTY_SLOT {
            return false;
        }
        if self.slots[slot].is_none() {
            self.used += 1;
        }
        self.slots[slot] = Some(rgb);
        true
    }

    pub fn get(&self, slot: usize) -> Option<u32> {
        self.slots.get(slot).copied().flatten()
    }

    pub fn used(&self) -> usize {
        self.used
    }
}

fn palette_checksum(bytes: &[u8]) -> u32 {
    // FNV-1a 32 位（与 pebind 记录校验和同族设施——一处一事实：算法一处定义）。
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 序列化（写盘模型：魔数 + 槽位 + 校验和；截断缓冲如实返回已写字节数）。
pub fn serialize_palette(p: &CustomPalette, buf: &mut [u8]) -> usize {
    let mut tmp = [0u8; PALETTE_SERIAL_SIZE];
    tmp[..4].copy_from_slice(&PALETTE_MAGIC);
    for i in 0..CUSTOM_COLOR_SLOTS {
        let v = p.get(i).unwrap_or(PALETTE_EMPTY_SLOT);
        tmp[4 + i * 4..8 + i * 4].copy_from_slice(&v.to_le_bytes());
    }
    let sum = palette_checksum(&tmp[..PALETTE_SERIAL_SIZE - 4]);
    tmp[PALETTE_SERIAL_SIZE - 4..].copy_from_slice(&sum.to_le_bytes());
    let n = buf.len().min(PALETTE_SERIAL_SIZE);
    buf[..n].copy_from_slice(&tmp[..n]);
    n
}

/// 反序列化（损坏即拒——魔数/尺寸/校验和三关，坏盘不静默吞）。
pub fn deserialize_palette(data: &[u8]) -> Result<CustomPalette, &'static str> {
    if data.len() < PALETTE_SERIAL_SIZE {
        return Err("palette: truncated");
    }
    if data[..4] != PALETTE_MAGIC {
        return Err("palette: bad magic");
    }
    let body = &data[..PALETTE_SERIAL_SIZE - 4];
    let stored = u32::from_le_bytes([
        data[PALETTE_SERIAL_SIZE - 4],
        data[PALETTE_SERIAL_SIZE - 3],
        data[PALETTE_SERIAL_SIZE - 2],
        data[PALETTE_SERIAL_SIZE - 1],
    ]);
    if palette_checksum(body) != stored {
        return Err("palette: checksum mismatch");
    }
    let mut p = CustomPalette::new();
    for i in 0..CUSTOM_COLOR_SLOTS {
        let off = 4 + i * 4;
        let v = u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        if v != PALETTE_EMPTY_SLOT {
            p.set(i, v);
        }
    }
    Ok(p)
}

/// 对话框键盘焦点（Tab 循环序 = 视觉序：文件名 → 过滤器 → 列表 → 确定 → 取消）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogFocus {
    Filename,
    Filter,
    FileList,
    Ok,
    Cancel,
}

/// Tab 焦点循环序（一处一事实：与视觉焦点序一致）。
pub const FOCUS_CYCLE: [DialogFocus; 5] = [
    DialogFocus::Filename,
    DialogFocus::Filter,
    DialogFocus::FileList,
    DialogFocus::Ok,
    DialogFocus::Cancel,
];

/// 键盘导航状态机（Enter 确定 / Esc 取消——键盘用户与鼠标用户能力对等）。
#[derive(Clone, Copy, Debug)]
pub struct KbdNav {
    idx: usize,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl KbdNav {
    pub const fn new() -> KbdNav {
        KbdNav { idx: 0, confirmed: false, cancelled: false }
    }

    pub fn focus(&self) -> DialogFocus {
        FOCUS_CYCLE[self.idx]
    }

    /// Tab 下一焦点（循环；终态后仍可循环——对话框活着就能走）。
    pub fn tab(&mut self) {
        self.idx = (self.idx + 1) % FOCUS_CYCLE.len();
    }

    /// Enter：Ok 焦点 = 确定；Cancel 焦点 = 取消；其余焦点 = 移动到 Ok 的
    /// 缺省确认（Windows 缺省按钮语义）。终态后 Enter 不重复生效。
    pub fn enter(&mut self) -> bool {
        if self.confirmed || self.cancelled {
            return false;
        }
        match self.focus() {
            DialogFocus::Ok => {
                self.confirmed = true;
                true
            }
            DialogFocus::Cancel => {
                self.cancelled = true;
                true
            }
            _ => {
                self.confirmed = true;
                true
            }
        }
    }

    /// Esc：任何焦点可取消（终态后 Esc 不重复生效）。
    pub fn esc(&mut self) -> bool {
        if self.confirmed || self.cancelled {
            return false;
        }
        self.cancelled = true;
        true
    }
}

/// F008 深化批次三自检。
pub fn run_comdlg_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F008-comdlg-deep2");
    // 1) 色板持久化 round-trip：写 3 槽 → 序列化 → 反序列化逐槽一致；
    //    空槽保持空；used 计数不虚。
    let mut p = CustomPalette::new();
    p.set(0, 0x00FF_0000);
    p.set(5, 0x0000_FF00);
    p.set(15, 0x00FF_FF00);
    let mut buf = [0u8; PALETTE_SERIAL_SIZE];
    serialize_palette(&p, &mut buf);
    let back = deserialize_palette(&buf);
    let rt = match back {
        Ok(q) => {
            q.get(0) == Some(0x00FF_0000)
                && q.get(5) == Some(0x0000_FF00)
                && q.get(15) == Some(0x00FF_FF00)
                && q.get(1).is_none()
                && q.used() == 3
        }
        None => false,
    };
    cs.add("palette_roundtrip", rt && p.used() == 3, "");
    // 2) 色板三关拒绝：坏魔数 / 截断 / 校验和翻转——坏盘不静默吞。
    let mut corrupt = buf;
    corrupt[8] ^= 0x01;
    cs.add(
        "palette_triple_gate_reject",
        deserialize_palette(&buf[..8]).is_err()
            && matches!(deserialize_palette(&[0u8; PALETTE_SERIAL_SIZE]), Err("palette: bad magic"))
            && deserialize_palette(&corrupt).is_err(),
        "",
    );
    // 3) 键盘状态机：Tab 五焦点循环回到起点；Enter 确认终态；Esc 任何焦点取消；
    //    终态后 Enter/Esc 不重复生效。
    let mut nav = KbdNav::new();
    let mut wrapped = true;
    for _ in 0..FOCUS_CYCLE.len() {
        nav.tab();
    }
    wrapped &= nav.focus() == FOCUS_CYCLE[0];
    nav.esc();
    let esc_again = nav.esc();
    let enter_after = nav.enter();
    let mut nav2 = KbdNav::new();
    let enter_ok = nav2.enter();
    cs.add(
        "kbd_nav_full_cycle_and_terminals",
        wrapped
            && nav.cancelled
            && !esc_again
            && !enter_after
            && enter_ok
            && nav2.confirmed,
        "",
    );
    cs
}
