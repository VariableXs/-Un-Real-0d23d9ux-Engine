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

/// 引导菜单关心的键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
}

/// 扫描码 → 键。返回 `None` 表示与本菜单无关（含断码/其它键）。
/// `_ext` 为 true 表示前一个字节是 0xE0（扩展前缀）；控制器翻译后扩展
/// 方向键/Enter 的通码数值与主键一致，故此处无需区分。
pub fn decode(make: u8, _ext: bool) -> Option<Key> {
    match make {
        0x48 => Some(Key::Up),
        0x50 => Some(Key::Down),
        0x1C => Some(Key::Enter),
        _ => None,
    }
}

/// 有状态的扫描码解码器：消化 0xE0 前缀与断码，产出 `Key`。
#[derive(Default)]
pub struct Decoder {
    pending_ext: bool,
}

impl Decoder {
    /// 喂入一个来自 0x60 的原始字节，得到 0 或 1 个键。
    pub fn feed(&mut self, b: u8) -> Option<Key> {
        if b == 0xE0 {
            self.pending_ext = true;
            return None;
        }
        let ext = core::mem::take(&mut self.pending_ext);
        if b & 0x80 != 0 {
            return None; // 断码：菜单只关心按下
        }
        decode(b, ext)
    }
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
}
