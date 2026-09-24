//! composesw — WP-202 · B-904 组合期分流（MD2 篇 9.4）。
//!
//! 判据 B-904：组合期快捷键零误触（判例集）。
//! MD2 原文（9.4）："IME 组合期（缓冲非空）的键盘事件一律进引擎不进
//! 仲裁——组合期触发快捷键是经典误触（宪章第六章），在仲裁入口直接
//! 分流。"施工要点补充："要用真人输入法过、不能用模拟键过——模拟键没有
//! IME 的组合状态，测不出真问题"（实机判例项，随队跟踪）。
//!
//! 宿主可测形态：IME 组合状态（缓冲空/非空）+ **仲裁入口直接分流**
//! （组合期事件 100% 进引擎、非组合期 100% 进仲裁——分流先于任何快捷键
//! 匹配发生）+ 分流与 B-901 单向流兼容（引擎是过滤器消费面，不改写事件）。

use crate::checks::CheckSet;

/// IME 组合缓冲（组合态的判定源：缓冲非空 = 组合期）。
pub struct ImeBuffer {
    /// 缓冲中的字母（物理键位序列的字母面模型——大写化前）。
    pub letters: [u8; 32],
    pub n: usize,
}

impl ImeBuffer {
    pub const fn new() -> ImeBuffer {
        ImeBuffer { letters: [0; 32], n: 0 }
    }

    pub const fn composing(&self) -> bool {
        self.n > 0
    }

    /// 键入进缓冲（组合期字母累计）。
    pub fn push_letter(&mut self, b: u8) -> bool {
        if self.n >= 32 {
            return false;
        }
        self.letters[self.n] = b;
        self.n += 1;
        true
    }

    /// 退格（缓冲层编辑——永不污染应用窗口）。
    pub fn backspace(&mut self) -> bool {
        if self.n == 0 {
            return false;
        }
        self.n -= 1;
        true
    }

    /// 上屏提交后清空缓冲（组合期结束）。
    pub fn commit_clear(&mut self) {
        self.n = 0;
    }
}

/// 仲裁入口的路由结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// 组合期：事件进引擎（不进快捷键仲裁）。
    Ime,
    /// 非组合期：事件进快捷键仲裁。
    Hotkey,
}

/// 仲裁入口分流（MD2 9.4：分流发生在仲裁入口——先于任何快捷键匹配）。
pub const fn arbiter_gate(composing: bool) -> Route {
    if composing {
        Route::Ime
    } else {
        Route::Hotkey
    }
}

/// 分流 + 引擎消费的组合行为（组合期事件进引擎后的缓冲动作）。
pub fn engine_consume(buf: &mut ImeBuffer, scancode: u16) -> Route {
    let r = arbiter_gate(buf.composing());
    if r == Route::Ime {
        // 字母面键入进缓冲；退格（scancode 模型值 14 = Backspace 物理位）在缓冲层编辑
        if scancode == 14 {
            let _ = buf.backspace();
        } else {
            let _ = buf.push_letter((scancode % 26 + 'a' as u16) as u8);
        }
    }
    r
}

// ---------------------------------------------------------------- 对练

/// 分流对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct SwDrillSummary {
    pub rounds: u32,
    pub ime_routed: u64,
    pub hotkey_routed: u64,
    /// 组合期事件 100% 进引擎（零误触的语义面）
    pub compose_all_ime: bool,
    /// 非组合期事件 100% 进仲裁
    pub idle_all_hotkey: bool,
}

/// 随机组合态 × 随机事件对练：分流完备性。
pub fn run_sw_drills(seed: u64, rounds: u32) -> SwDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = SwDrillSummary::default();
    sum.rounds = rounds;
    sum.compose_all_ime = true;
    sum.idle_all_hotkey = true;
    let mut buf = ImeBuffer::new();
    for i in 0..rounds {
        // 随机组合态操作：键入/退格/提交
        match g.next() % 3 {
            0 => {
                let _ = buf.push_letter((g.next() % 26 + 'a' as u64) as u8);
            }
            1 => {
                let _ = buf.backspace();
            }
            _ => {
                if i % 4 == 3 {
                    buf.commit_clear(); // 周期性上屏，保证空/非空两态都出现
                }
            }
        }
        let sc = (g.next() % 256) as u16;
        // 断言口径：路由裁决基于**进入时**的组合态（engine_consume 内部
        // 会修改缓冲——退格清空后 composing 变 false 不代表路由错误）
        let was_composing = buf.composing();
        let r = engine_consume(&mut buf, sc);
        match r {
            Route::Ime => {
                sum.ime_routed += 1;
                if !was_composing {
                    // 进引擎的路由必须发生在组合期（进入时缓冲非空）
                    sum.compose_all_ime = false;
                }
            }
            Route::Hotkey => {
                sum.hotkey_routed += 1;
                if was_composing {
                    sum.idle_all_hotkey = false;
                }
            }
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_composesw_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-904 组合期分流");
    {
        // 组合态判定：缓冲空/非空
        let mut buf = ImeBuffer::new();
        let idle = !buf.composing();
        let _ = buf.push_letter(b'n');
        let composing = buf.composing();
        set.add(
            "B-904 组合态判定",
            idle && composing,
            "IME 组合期（缓冲非空）——MD2 9.4",
        );
    }
    {
        // 分流：组合期进引擎
        let mut buf = ImeBuffer::new();
        let _ = buf.push_letter(b'n');
        set.add(
            "B-904 组合期进引擎",
            arbiter_gate(buf.composing()) == Route::Ime,
            "组合期键盘事件一律进引擎不进仲裁",
        );
    }
    {
        // 分流：非组合期进仲裁
        let buf = ImeBuffer::new();
        set.add(
            "B-904 非组合期进仲裁",
            arbiter_gate(buf.composing()) == Route::Hotkey,
            "空缓冲事件正常走快捷键仲裁",
        );
    }
    {
        // 组合期零误触：任意 scancode 在组合期都不触发仲裁
        let mut buf = ImeBuffer::new();
        let _ = buf.push_letter(b'i');
        let all_ime = (0..256u16).all(|sc| arbiter_gate(buf.composing()) == Route::Ime);
        set.add(
            "B-904 组合期零误触（穷举）",
            all_ime,
            "组合期 256 键位全部进引擎——经典误触在入口分流",
        );
    }
    {
        // 缓冲层编辑：退格不污染应用窗口
        let mut buf = ImeBuffer::new();
        let _ = buf.push_letter(b'z');
        let _ = buf.push_letter(b'h');
        let ok = buf.backspace() && buf.n == 1 && buf.composing();
        let _ = buf.commit_clear();
        set.add(
            "B-904 缓冲层编辑不污染窗口",
            ok && !buf.composing(),
            "输入串编辑在缓冲层完成（上屏前应用不见组合中间态）",
        );
    }
    {
        // 引擎消费闭环：键入组合 → 组合期路由 → 上屏清空 → 恢复仲裁
        let mut buf = ImeBuffer::new();
        let _ = buf.push_letter(b'f');          // IME 激活：首字母进缓冲（组合开始）
        let r1 = engine_consume(&mut buf, 33);  // 组合期（缓冲非空）→ Ime
        let r2 = engine_consume(&mut buf, 14);  // 组合期退格 → Ime
        buf.commit_clear();                     // 上屏清空
        let r3 = engine_consume(&mut buf, 33);  // 空闲 → Hotkey
        set.add(
            "B-904 引擎消费闭环",
            r1 == Route::Ime && r2 == Route::Ime && r3 == Route::Hotkey,
            "键入组合 → 组合期路由 → 上屏清空 → 恢复仲裁路由",
        );
    }
    {
        // 分流在仲裁入口（先于快捷键匹配）——结构面：gate 只依赖组合态，不依赖键值
        set.add(
            "B-904 入口分流结构面",
            arbiter_gate(true) == Route::Ime && arbiter_gate(false) == Route::Hotkey,
            "gate 签名只收组合态——任何键值在组合期都到不了快捷键匹配",
        );
    }
    {
        // 分流对练
        let sum = run_sw_drills(0xB904, 80);
        set.add(
            "B-904 分流对练",
            sum.rounds == 80 && sum.compose_all_ime && sum.idle_all_hotkey && sum.ime_routed > 0 && sum.hotkey_routed > 0,
            "组合期快捷键零误触（判据原文的宿主语义面）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f804_gate_two_way() {
        assert_eq!(arbiter_gate(true), Route::Ime);
        assert_eq!(arbiter_gate(false), Route::Hotkey);
    }

    #[test]
    fn f804_buffer_lifecycle() {
        let mut buf = ImeBuffer::new();
        assert!(!buf.composing());
        let _ = buf.push_letter(b'a');
        let _ = buf.push_letter(b'b');
        assert!(buf.composing() && buf.n == 2);
        assert!(buf.backspace());
        assert_eq!(buf.n, 1);
        buf.commit_clear();
        assert!(!buf.composing());
        assert!(!buf.backspace(), "空缓冲退格无效");
    }

    #[test]
    fn f804_zero_false_trigger() {
        let mut buf = ImeBuffer::new();
        let _ = buf.push_letter(b'x');
        for sc in 0..256u16 {
            assert_eq!(engine_consume(&mut buf, sc), Route::Ime, "组合期任意键不进仲裁");
        }
    }

    #[test]
    fn f804_drill_deterministic() {
        let a = run_sw_drills(7, 40);
        let b = run_sw_drills(7, 40);
        assert_eq!(a, b);
        assert!(a.compose_all_ime && a.idle_all_hotkey);
        assert!(a.ime_routed > 0 && a.hotkey_routed > 0);
    }
}
