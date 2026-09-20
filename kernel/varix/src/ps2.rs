//! PS/2 键盘轮询 — 引导期键事件查询接口（双域总案·阶段0 任务1）。
//!
//! 职责边界（刻意保持最小）：
//! - 只做 **轮询**：读 0x64 状态 / 0x60 数据口，把 Set-1 扫描码解码为少量
//!   引导菜单需要的键（↑/↓/Enter）。中断、缓冲队列、hid 域接线属于任务 19。
//! - 解码逻辑（`decode`/`Decoder`）与端口 IO（`poll_key`）严格分离：前者
//!   纯函数宿主可测，后者仅 `target_os = "none"` 编译。
//! - 引导期单核、中断未开，轮询即正确；无键盘/控制器缺失时接口如实返回
//!   `None`，绝不自旋卡死（读口有 guard 上限）。
//!
//! 扫描码事实（QEMU/常见 PS/2 键盘，第一套翻译后）：
//! - 0x48 ↑ / 0x50 ↓ / 0x1C Enter；断码 = 通码 | 0x80（忽略）。
//! - 扩展键前缀 0xE0（E0 48 / E0 50 / E0 1C），由 `Decoder` 状态机消化。

/// 键词汇表。任务55（AI-V）扩展：在引导菜单三键（Up/Down/Enter，契约
/// 序号 0/1/2 恒定不变）之上扩出 shell/IME 需要的全量主键区——
/// 方向/编辑键 + A-Z + 数字排 + 常用标点。扩展只加变体不改既有判别值
/// （inputsvc::key_byte 是唯一序号来源，注入层同表同源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Space,
    Backspace,
    Tab,
    LShift,
    RShift,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    D1,
    D2,
    D3,
    D4,
    D5,
    D6,
    D7,
    D8,
    D9,
    D0,
    Minus,
    Equal,
    Comma,
    Period,
    Slash,
    Semicolon,
    Apostrophe,
    BracketL,
    BracketR,
    Backslash,
    Grave,
}

/// 扫描码 → 键。返回 `None` 表示与本层无关（含断码/其它键）。
/// `_ext` 为 true 表示前一个字节是 0xE0（扩展前缀）：扩展区承载
/// Left/Right（主区 0x4B/0x4D 是小键盘键，不认）；主区 Up/Down 沿用
/// 任务1 事实（QEMU 翻译层两种形态都出现过，保持宽容）。
pub fn decode(make: u8, ext: bool) -> Option<Key> {
    if ext {
        return match make {
            0x48 => Some(Key::Up),
            0x50 => Some(Key::Down),
            0x4B => Some(Key::Left),
            0x4D => Some(Key::Right),
            0x1C => Some(Key::Enter), // 小键盘 Enter / 扩展 Enter
            _ => None,
        };
    }
    match make {
        0x48 => Some(Key::Up),
        0x50 => Some(Key::Down),
        0x4B => Some(Key::Left),
        0x4D => Some(Key::Right),
        0x1C => Some(Key::Enter),
        0x01 => Some(Key::Esc),
        0x39 => Some(Key::Space),
        0x0E => Some(Key::Backspace),
        0x0F => Some(Key::Tab),
        0x2A => Some(Key::LShift),
        0x36 => Some(Key::RShift),
        0x10 => Some(Key::Q),
        0x11 => Some(Key::W),
        0x12 => Some(Key::E),
        0x13 => Some(Key::R),
        0x14 => Some(Key::T),
        0x15 => Some(Key::Y),
        0x16 => Some(Key::U),
        0x17 => Some(Key::I),
        0x18 => Some(Key::O),
        0x19 => Some(Key::P),
        0x1E => Some(Key::A),
        0x1F => Some(Key::S),
        0x20 => Some(Key::D),
        0x21 => Some(Key::F),
        0x22 => Some(Key::G),
        0x23 => Some(Key::H),
        0x24 => Some(Key::J),
        0x25 => Some(Key::K),
        0x26 => Some(Key::L),
        0x2C => Some(Key::Z),
        0x2D => Some(Key::X),
        0x2E => Some(Key::C),
        0x2F => Some(Key::V),
        0x30 => Some(Key::B),
        0x31 => Some(Key::N),
        0x32 => Some(Key::M),
        0x02 => Some(Key::D1),
        0x03 => Some(Key::D2),
        0x04 => Some(Key::D3),
        0x05 => Some(Key::D4),
        0x06 => Some(Key::D5),
        0x07 => Some(Key::D6),
        0x08 => Some(Key::D7),
        0x09 => Some(Key::D8),
        0x0A => Some(Key::D9),
        0x0B => Some(Key::D0),
        0x0C => Some(Key::Minus),
        0x0D => Some(Key::Equal),
        0x33 => Some(Key::Comma),
        0x34 => Some(Key::Period),
        0x35 => Some(Key::Slash),
        0x27 => Some(Key::Semicolon),
        0x28 => Some(Key::Apostrophe),
        0x1A => Some(Key::BracketL),
        0x1B => Some(Key::BracketR),
        0x2B => Some(Key::Backslash),
        0x29 => Some(Key::Grave),
        _ => None,
    }
}

/// 键 → ASCII 字符（IME/文本路径用）。Shift 语义只对可移位键生效；
/// 返回 `None` = 非字符键（方向/Enter/Esc/修饰）。
pub fn to_char(k: Key, shift: bool) -> Option<u8> {
    use Key::*;
    let lower = match k {
        A => b'a',
        B => b'b',
        C => b'c',
        D => b'd',
        E => b'e',
        F => b'f',
        G => b'g',
        H => b'h',
        I => b'i',
        J => b'j',
        K => b'k',
        L => b'l',
        M => b'm',
        N => b'n',
        O => b'o',
        P => b'p',
        Q => b'q',
        R => b'r',
        S => b's',
        T => b't',
        U => b'u',
        V => b'v',
        W => b'w',
        X => b'x',
        Y => b'y',
        Z => b'z',
        D1 => b'1',
        D2 => b'2',
        D3 => b'3',
        D4 => b'4',
        D5 => b'5',
        D6 => b'6',
        D7 => b'7',
        D8 => b'8',
        D9 => b'9',
        D0 => b'0',
        Minus => b'-',
        Equal => b'=',
        Comma => b',',
        Period => b'.',
        Slash => b'/',
        Semicolon => b';',
        Apostrophe => b'\'',
        BracketL => b'[',
        BracketR => b']',
        Backslash => b'\\',
        Grave => b'`',
        Space => b' ',
        _ => return None,
    };
    if !shift {
        return Some(lower);
    }
    Some(match lower {
        b'a'..=b'z' => lower - 32,
        b'1' => b'!',
        b'2' => b'@',
        b'3' => b'#',
        b'4' => b'$',
        b'5' => b'%',
        b'6' => b'^',
        b'7' => b'&',
        b'8' => b'*',
        b'9' => b'(',
        b'0' => b')',
        b'-' => b'_',
        b'=' => b'+',
        b',' => b'<',
        b'.' => b'>',
        b'/' => b'?',
        b';' => b':',
        b'\'' => b'"',
        b'[' => b'{',
        b']' => b'}',
        b'\\' => b'|',
        b'`' => b'~',
        other => other,
    })
}

/// 有状态的扫描码解码器：消化 0xE0 前缀与断码，产出 `Key`。
#[derive(Default)]
pub struct Decoder {
    pending_ext: bool,
    pending_break: bool,
}

impl Decoder {
    /// 喂入一个来自 0x60 的原始字节，得到 0 或 1 个键。
    ///
    /// 解码顺序（2026-09-20 实机戒律）：先 SET1，无匹配再走 SET2 导航兜底。
    /// 真机笔记本（Y7000 EC/i8042 兼容层）可能发 SET2 扫描码（Enter=0x5A、
    /// ↑=0x75），QEMU 的 sendkey 走 SET1——只认 SET1 时真机「press any key」
    /// 全部静默丢失。SET2 断码=0xF0 前缀，须消化掉防松键误读成按下。
    pub fn feed(&mut self, b: u8) -> Option<Key> {
        if b == 0xE0 {
            self.pending_ext = true;
            return None;
        }
        if b == 0xF0 {
            // SET2 断码前缀：下一个字节是 make code，丢弃整对。
            self.pending_break = true;
            return None;
        }
        if self.pending_break {
            self.pending_break = false;
            return None; // SET2 断码的 make 部分：松键，不产事件
        }
        let ext = core::mem::take(&mut self.pending_ext);
        if b & 0x80 != 0 {
            return None; // SET1 断码：菜单只关心按下
        }
        decode(b, ext).or_else(|| if ext { None } else { decode_set2_nav(b) })
    }
}

/// SET2 导航/常用键兜底（仅收录与 SET1 表不冲突的 make code）。
/// SET1 已映射的字节（如 0x0D=Equal、0x29=Grave）永远先走 SET1——
/// 兜底只补 SET1 空位，QEMU/标准键盘的 SET1 行为零变化。
fn decode_set2_nav(b: u8) -> Option<Key> {
    Some(match b {
        0x75 => Key::Up,        // SET2 ↑
        0x72 => Key::Down,      // SET2 ↓
        0x6B => Key::Left,      // SET2 ←
        0x74 => Key::Right,     // SET2 →
        0x5A => Key::Enter,     // SET2 Enter
        0x76 => Key::Esc,       // SET2 Esc
        0x66 => Key::Backspace, // SET2 Bksp
        _ => return None,
    })
}

/// 目标态：从 PS/2 控制器轮询出一个键事件（非阻塞）。
/// 控制器缺失/超时返回 `None`，绝不挂死引导。
#[cfg(target_os = "none")]
pub(crate) mod port {
    /// 端口读 — `in al, dx`（与 serial::io 同范式；任务19 起开放给
    /// inputsvc 泵复用，保持端口 IO 单一来源）。
    #[inline]
    pub unsafe fn inp(port: u16) -> u8 {
        let val: u8;
        unsafe {
            core::arch::asm!(
                "in al, dx",
                out("al") val,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            )
        }
        val
    }

    /// 端口写 — `out dx, al`（任务19 鼠标 bring-up 用）。
    #[inline]
    pub unsafe fn outp(port: u16, val: u8) {
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("al") val,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            )
        }
    }
}

#[cfg(target_os = "none")]
pub fn poll_key() -> Option<Key> {
    use port::inp;
    const STAT: u16 = 0x64;
    const DATA: u16 = 0x60;
    const STAT_OBF: u8 = 0x01;

    unsafe {
        // 控制器存在性 guard：状态口读回 0xFF 视为无控制器（浮空总线）。
        let st = inp(STAT);
        if st == 0xFF {
            return None;
        }
        if st & STAT_OBF == 0 {
            return None;
        }
        let b = inp(DATA);
        static mut DECODER: Option<Decoder> = None;
        // 引导期单核，static mut 无并发；Once 语义手工维护。
        let dec = {
            let slot = &raw mut DECODER;
            if (*slot).is_none() {
                *slot = Some(Decoder::default());
            }
            (*slot).as_mut().unwrap()
        };
        dec.feed(b)
    }
}

/// 宿主态：没有端口可读，恒 `None`（菜单在宿主测试里用注入键源）。
#[cfg(not(target_os = "none"))]
pub fn poll_key() -> Option<Key> {
    None
}

// ---------------------------------------------------------------------------
// 实机 i8042 控制器初始化 + 取证（2026-09-20 实机戒律）
// ---------------------------------------------------------------------------

/// 探针字打包（纯函数，宿主测试与目标态共用；ushell 诊断行按同一位格式
/// 解码——独立 crate 零链接，双源对照）：
/// bit0=控制器在位，bit2:1=控制器自检结果，bit4:3=键盘接口测试结果。
/// 结果码 1=OK 2=超时 3=异常响应，0=未执行（控制器不在位）。
pub fn pack_probe(present: bool, selftest: u8, iface: u8) -> u32 {
    (present as u32) | ((selftest as u32 & 0x3) << 1) | ((iface as u32 & 0x3) << 3)
}

/// 从探针字取回 (present, selftest, iface)。
pub fn unpack_probe(word: u32) -> (bool, u8, u8) {
    (
        word & 0x1 != 0,
        ((word >> 1) & 0x3) as u8,
        ((word >> 3) & 0x3) as u8,
    )
}

#[cfg(target_os = "none")]
static KBD_PROBE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// 目标态：读回控制器探针字（input_probe 启动期 `controller_init` 写入）。
#[cfg(target_os = "none")]
pub fn probe_word() -> u32 {
    KBD_PROBE.load(core::sync::atomic::Ordering::Acquire)
}

/// 启动期一次性 i8042 初始化（真机必须，QEMU 容忍）。
///
/// 2026-09-20 实机戒律：固件移交后的控制器状态不可假设——真机 BootScreen
/// 按键全无响应的根因候选就是控制器未被留在可用态（QEMU 开箱即用复现
/// 不了这一点）。标准序列：关两端口→清输出→控制器自检(0xAA→0x55)→键盘
/// 接口测试(0xAB→0x00)→配置回写（**保留翻译位 bit6**：固件怎么配就怎么用，
/// SET1/SET2 解码两侧都兜住；清 IRQ0/IRQ12 进轮询模式；清时钟禁用位=
/// 开两端口）→开键盘端口(0xAE)→再清输出。每步限次自旋，绝不挂死引导。
#[cfg(target_os = "none")]
pub fn controller_init() {
    use port::{inp, outp};
    const STAT: u16 = 0x64;
    const DATA: u16 = 0x60;
    const STAT_OBF: u8 = 0x01;
    const STAT_IBF: u8 = 0x02;

    // 等输入缓冲空（IBF=0）再发命令/数据。
    let send = |c: u8| -> bool {
        for _ in 0..200_000 {
            unsafe {
                if inp(STAT) & STAT_IBF == 0 {
                    outp(STAT, c);
                    return true;
                }
            }
            core::hint::spin_loop();
        }
        false
    };
    // 等输出缓冲有数据（限次自旋）后读一字节。
    let resp = || -> Option<u8> {
        for _ in 0..2_000_000 {
            unsafe {
                if inp(STAT) & STAT_OBF != 0 {
                    return Some(inp(DATA));
                }
            }
            core::hint::spin_loop();
        }
        None
    };
    // 清输出残留（有数据就读掉，最多 64 字节）。
    let flush = || {
        for _ in 0..64 {
            unsafe {
                if inp(STAT) & STAT_OBF == 0 {
                    return;
                }
                let _ = inp(DATA);
            }
        }
    };

    let st = unsafe { inp(STAT) };
    if st == 0xFF {
        // 浮空总线=无控制器：探针字归零，轮询侧自会静默。
        KBD_PROBE.store(0, core::sync::atomic::Ordering::Release);
        return;
    }
    let _ = send(0xAD); // 关第一端口
    let _ = send(0xA7); // 关第二端口
    flush();
    let st_code = if send(0xAA) {
        match resp() {
            Some(0x55) => 1,
            Some(_) => 3,
            None => 2,
        }
    } else {
        2
    };
    let if_code = if send(0xAB) {
        match resp() {
            Some(0x00) => 1,
            Some(_) => 3,
            None => 2,
        }
    } else {
        2
    };
    flush(); // 自检会复位控制器，重清残留
    if send(0x20) {
        let cfg = resp().unwrap_or(0);
        // 保留 bit6 翻译位；清 IRQ0(0x01)/IRQ12(0x02)；清时钟禁用(0x10/0x20)=开两端口。
        let cfg2 = cfg & !(0x01 | 0x02 | 0x10 | 0x20);
        if send(0x60) {
            unsafe { outp(DATA, cfg2) };
        }
    }
    let _ = send(0xAE); // 开第一端口（键盘）
    flush();
    KBD_PROBE.store(
        pack_probe(true, st_code, if_code),
        core::sync::atomic::Ordering::Release,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_menu_keys() {
        assert_eq!(decode(0x48, false), Some(Key::Up));
        assert_eq!(decode(0x50, false), Some(Key::Down));
        assert_eq!(decode(0x1C, false), Some(Key::Enter));
        assert_eq!(decode(0x1D, false), None); // Ctrl 等无关键
        assert_eq!(decode(0x48, true), Some(Key::Up)); // 扩展 ↑ 同值
    }

    #[test]
    fn decoder_handles_ext_prefix_and_breaks() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(0xE0), None);
        assert_eq!(d.feed(0x50), Some(Key::Down));
        assert_eq!(d.feed(0x50 | 0x80), None); // 断码
        assert_eq!(d.feed(0x1C), Some(Key::Enter));
        // 前缀后跟断码：前缀被消化，不污染后续通码
        assert_eq!(d.feed(0xE0), None);
        assert_eq!(d.feed(0x48 | 0x80), None);
        assert_eq!(d.feed(0x48), Some(Key::Up));
    }

    #[test]
    fn decoder_reset_after_prefix() {
        let mut d = Decoder::default();
        assert_eq!(d.feed(0xE0), None);
        assert_eq!(d.feed(0x2A), None); // Shift 通码，消化掉前缀
        assert_eq!(d.feed(0x48), Some(Key::Up)); // 不再处于扩展态
    }

    /// 实机戒律（2026-09-20）：Y7000 的 EC/i8042 兼容层可能发 SET2 扫描码，
    /// 只认 SET1 时真机 BootScreen「press any key」全部静默丢失。
    /// 锁死：SET2 导航键兜底 + 0xF0 断码对消化 + SET1 语义零变化。
    #[test]
    fn decoder_set2_fallback_for_real_laptops() {
        let mut d = Decoder::default();
        // SET2 导航/常用 make codes
        assert_eq!(d.feed(0x5A), Some(Key::Enter), "SET2 Enter=0x5A");
        assert_eq!(d.feed(0x75), Some(Key::Up), "SET2 Up=0x75");
        assert_eq!(d.feed(0x72), Some(Key::Down), "SET2 Down=0x72");
        assert_eq!(d.feed(0x6B), Some(Key::Left), "SET2 Left=0x6B");
        assert_eq!(d.feed(0x74), Some(Key::Right), "SET2 Right=0x74");
        assert_eq!(d.feed(0x76), Some(Key::Esc), "SET2 Esc=0x76");
        // SET2 断码对（0xF0 + make）整体消化，松键不产事件
        assert_eq!(d.feed(0xF0), None, "SET2 断码前缀");
        assert_eq!(d.feed(0x5A), None, "断码 make 部分被消化");
        // SET1 语义零变化（QEMU sendkey 路径回归）
        assert_eq!(d.feed(0x1C), Some(Key::Enter), "SET1 Enter 不变");
        assert_eq!(d.feed(0x50), Some(Key::Down), "SET1 Down 不变");
    }

    /// 实机探针字位格式（2026-09-20）：pack/unpack 往返一致；高 24 位
    /// （原始字节计数+最后字节，inputsvc::kbd_diag_word 复用同一 u32）
    /// 不受低 8 位探针字段影响。
    #[test]
    fn kbd_probe_word_roundtrip() {
        for &(present, st, ifc) in
            &[(true, 1u8, 1u8), (true, 2, 3), (true, 3, 2), (false, 0, 0)]
        {
            let w = pack_probe(present, st, ifc);
            assert_eq!(unpack_probe(w), (present, st, ifc));
        }
        let w = pack_probe(true, 1, 1) | (0x1234 << 8) | (0x7F << 24);
        assert_eq!(unpack_probe(w), (true, 1, 1), "高 24 位不溅入探针字段");
    }
}
