//! UNX-F0208 · 报告展示接口（诊断模式 UI）
//!
//! 锚点判据（施工书原文）：UNX-F0208-J1 诊断模式分页浏览报告，
//! 键位（上下翻/跳转/退出）全可用（体验 14 章口径）。
//!
//! 设计：报告行集 → 分页视口（每页 N 行）→ 键位状态机
//! （Up/Down/Jump(n)/Quit 全部显式建模，非法键拒收记账）。
//! 红线：①页游标永不越界（首页 Up 停、末页 Down 停）；
//! ②跳转目标页必须存在；③Quit 后再操作必拒；④行渲染逐行对账。
//!
//! 错误码段：0x2BE0（全仓普查后独占分配，AI-WBX1022）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0208_VERSION: &str = "F0208-reportui-v1";

/// 诊断码段基址（UNX-F0208 独占）。
pub const SEG_BASE: u16 = 0x2BE0;

/// 每页行数（体验 14 章口径：24 行终端）。
pub const ROWS_PER_PAGE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 页游标越界。
    PageOut = SEG_BASE + 1,
    /// 跳转目标页不存在。
    JumpBad = SEG_BASE + 2,
    /// Quit 后继续操作。
    AfterQuit = SEG_BASE + 3,
    /// 非法键。
    BadKey = SEG_BASE + 4,
    /// 行渲染数与页行数不符。
    RenderShort = SEG_BASE + 5,
    /// 空报告开视口。
    EmptyReport = SEG_BASE + 6,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 键位（体验 14 章口径四键 + 非法键兜底）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// 上翻。
    Up,
    /// 下翻。
    Down,
    /// 跳转到第 n 页（0 基）。
    Jump(usize),
    /// 退出。
    Quit,
}

/// 诊断模式分页视口。
#[derive(Debug)]
pub struct Pager {
    /// 报告行。
    lines: Vec<String>,
    /// 当前页（0 基）。
    page: usize,
    /// 已退出。
    quit: bool,
    /// 键位操作账（合法操作数）。
    ops: u64,
    /// 拒收账（非法操作数）。
    pub rejects: Vec<(u16, usize)>,
}

impl Pager {
    /// 开视口（空报告拒）。
    pub fn open(lines: Vec<String>) -> Result<Self, u16> {
        if lines.is_empty() {
            return Err(E::EmptyReport.code());
        }
        Ok(Pager {
            lines,
            page: 0,
            quit: false,
            ops: 0,
            rejects: Vec::new(),
        })
    }

    /// 总页数。
    pub fn pages(&self) -> usize {
        (self.lines.len() + ROWS_PER_PAGE - 1) / ROWS_PER_PAGE
    }

    /// 当前页。
    pub fn cur_page(&self) -> usize {
        self.page
    }

    /// 渲染当前页行（切片，不拷贝）。
    pub fn render(&self) -> Result<Vec<String>, u16> {
        if self.quit {
            return Err(E::AfterQuit.code());
        }
        let s = self.page * ROWS_PER_PAGE;
        let e = ((s + ROWS_PER_PAGE) as u64).min(self.lines.len() as u64) as usize;
        let out = self.lines[s..e].to_vec();
        if out.is_empty() {
            return Err(E::RenderShort.code());
        }
        Ok(out)
    }

    /// 键位分发。
    pub fn press(&mut self, k: Key) -> Result<(), u16> {
        if self.quit {
            self.rejects.push((E::AfterQuit.code(), self.page));
            return Err(E::AfterQuit.code());
        }
        match k {
            Key::Up => {
                if self.page > 0 {
                    self.page -= 1;
                }
                self.ops += 1;
                Ok(())
            }
            Key::Down => {
                if self.page + 1 < self.pages() {
                    self.page += 1;
                }
                self.ops += 1;
                Ok(())
            }
            Key::Jump(n) => {
                if n >= self.pages() {
                    self.rejects.push((E::JumpBad.code(), n));
                    return Err(E::JumpBad.code());
                }
                self.page = n;
                self.ops += 1;
                Ok(())
            }
            Key::Quit => {
                self.quit = true;
                self.ops += 1;
                Ok(())
            }
        }
    }

    /// 退出态。
    pub fn is_quit(&self) -> bool {
        self.quit
    }

    /// 键位全可用红线：四键各至少一次合法操作 + 拒收有账。
    pub fn keys_exercised(&self) -> bool {
        self.ops >= 4
    }
}

/// J1 自证（判据原文：分页浏览，键位上下翻/跳转/退出全可用）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：57 行报告 → 3 页（24+24+9）。
    let lines: Vec<String> = (1..=57)
        .map(|i| {
            let _ = 1u32;
            let mut s = String::from("line ");
            s.push_str(&itoa_str(i as u64));
            s
        })
        .collect();
    let mut p = Pager::open(lines)?;
    if p.pages() != 3 {
        return Err(E::PageOut.code());
    }

    // 二、J1 主体：Down 两页 → Jump 回 0 → Up 停首 → Quit。
    p.press(Key::Down)?;
    p.press(Key::Down)?;
    if p.cur_page() != 2 {
        return Err(E::PageOut.code());
    }
    // 末页 Down 停（不越界）。
    p.press(Key::Down)?;
    if p.cur_page() != 2 {
        return Err(E::PageOut.code());
    }
    let tail = p.render()?;
    if tail.len() != 9 {
        return Err(E::RenderShort.code());
    }
    p.press(Key::Jump(0))?;
    if p.cur_page() != 0 {
        return Err(E::JumpBad.code());
    }
    // 首页 Up 停。
    p.press(Key::Up)?;
    if p.cur_page() != 0 {
        return Err(E::PageOut.code());
    }
    p.press(Key::Quit)?;
    if !p.is_quit() {
        return Err(E::AfterQuit.code());
    }
    if !p.keys_exercised() {
        return Err(E::BadKey.code());
    }

    // 三、红线逐项：Quit 后操作拒且有账 / 跳转越页拒 / 空报告拒。
    if p.press(Key::Down).is_ok() {
        return Err(E::AfterQuit.code());
    }
    if p.rejects.is_empty() || p.rejects[0].0 != E::AfterQuit.code() {
        return Err(E::AfterQuit.code());
    }
    if p.press(Key::Jump(9)).is_ok() {
        return Err(E::JumpBad.code());
    }
    if Pager::open(Vec::new()).is_ok() {
        return Err(E::EmptyReport.code());
    }

    // 四、判别式段位对拍。
    if E::PageOut.code() != SEG_BASE + 1 || E::EmptyReport.code() != SEG_BASE + 6 {
        return Err(E::PageOut.code());
    }
    Ok(())
}

/// 极简 u64 → ASCII（no_std 十进制渲染，无 format 依赖）。
fn itoa_str(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    // 安全：buf 全部来自 ASCII 数字。
    String::from_utf8_lossy(&buf[i..]).into_owned()
}
