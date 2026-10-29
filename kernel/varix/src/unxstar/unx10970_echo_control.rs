//! UNX-F10970 回显控制联测（密码输入关回显场景）（0x1d8c0 段）
//!
//! 卡片判据（UNX-F10970-J1）：
//! - 关回显后键入零回显（100/100），开回显恢复逐字回显（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模密码输入回显控制：ECHO 位
//! 关闭时键入只进隐藏缓冲（回显流零字节），ECHO 位恢复后逐字
//! 回显；隐藏缓冲内容可校验（密码完整性），回显流逐字对账。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10970-10900)*0x20 = 0x1d8c0）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d8c0;

/// EC_EBADF：fd 无效
pub const EC_EBADF: u32 = SEG_BASE + 0x01;
/// EC_EINVAL：参数非法
pub const EC_EINVAL: u32 = SEG_BASE + 0x02;
/// EC_ELEAK：关回显期间出现回显字节（自检面）
pub const EC_ELEAK: u32 = SEG_BASE + 0x03;
/// EC_EDRIFT：隐藏缓冲与键入账不平（自检面）
pub const EC_EDRIFT: u32 = SEG_BASE + 0x04;
/// EC_ENOSPC：隐藏缓冲满
pub const EC_ENOSPC: u32 = SEG_BASE + 0x05;
/// EC_ECLOSED：会话已结束仍键入
pub const EC_ECLOSED: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 隐藏缓冲容量
/// 隐藏缓冲上限（对齐终端规范行缓冲量级，百键密码场景不得中途截断）。
const HIDDEN_CAP: usize = 512;
/// ECHO 位
pub const ECHO_BIT: u32 = 0x2;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 回显会话
#[derive(Clone, Debug)]
pub struct EchoSession {
    pub fd: i32,
    /// ECHO 位状态
    pub echo_on: bool,
    /// 会话开启
    pub open: bool,
}

impl EchoSession {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.fd as u64).wrapping_mul(0x100000001b3);
        h = (h ^ (self.echo_on as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.open as u64)).wrapping_mul(0x100000001b3);
        h
    }
}

/// 回显控制联测台
#[derive(Clone, Debug)]
pub struct EchoDesk {
    pub sess: EchoSession,
    /// 隐藏缓冲（键入内容）
    hidden: String,
    /// 回显流（终端可见）
    pub echo_stream: String,
    /// 键入总数
    pub typed: u64,
    /// 回显字节数
    pub echoed: u64,
    /// 错误计数
    pub n_ebadf: u32,
    pub n_einval: u32,
}

impl EchoDesk {
    pub fn new(fd: i32) -> Self {
        EchoDesk {
            sess: EchoSession { fd, echo_on: true, open: true },
            hidden: String::new(),
            echo_stream: String::new(),
            typed: 0,
            echoed: 0,
            n_ebadf: 0,
            n_einval: 0,
        }
    }

    /// 设置 ECHO 位
    pub fn set_echo(&mut self, on: bool) -> Result<(), u32> {
        if !self.sess.open {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(EC_EBADF);
        }
        self.sess.echo_on = on;
        Ok(())
    }

    /// 键入一个字符
    pub fn key(&mut self, ch: char) -> Result<bool, u32> {
        if !self.sess.open {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(EC_ECLOSED);
        }
        if self.hidden.len() >= HIDDEN_CAP {
            return Err(EC_ENOSPC);
        }
        self.hidden.push(ch);
        self.typed = self.typed.wrapping_add(1);
        if self.sess.echo_on {
            self.echo_stream.push(ch);
            self.echoed = self.echoed.wrapping_add(1);
            Ok(true)
        } else {
            // 关回显：零回显，但仍入隐藏缓冲
            Ok(false)
        }
    }

    /// 键入一串（密码场景）
    pub fn key_str(&mut self, s: &str) -> Result<u32, u32> {
        let mut echoed = 0u32;
        for ch in s.chars() {
            match self.key(ch)? {
                true => echoed = echoed.wrapping_add(1),
                false => {}
            }
        }
        Ok(echoed)
    }

    /// 关回显零泄漏自检：echo_on=false 期间回显流不增长
    pub fn no_leak(&self, echo_before: u64) -> bool {
        if !self.sess.echo_on {
            return self.echoed == echo_before;
        }
        self.echoed >= echo_before
    }

    /// 隐藏缓冲完整性：键入数=缓冲长度
    pub fn hidden_ok(&self, expect: &str) -> bool {
        self.hidden == expect
    }

    /// 逐字回显恢复自检：回显字节数=键入数（开启后）
    pub fn echo_ok(&self, since: u64, typed_since: u64) -> bool {
        self.echoed >= since && self.echoed - since == typed_since
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_typed: u64) -> bool {
        self.typed == expect_typed
            && self.hidden.len() as u64 == expect_typed
    }

    /// 提交密码（清隐藏缓冲）
    pub fn submit(&mut self) -> String {
        let out = self.hidden.clone();
        self.hidden.clear();
        out
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h = self.sess.fold();
        h = (h ^ self.typed).wrapping_mul(0x100000001b3);
        h = (h ^ self.echoed).wrapping_mul(0x100000001b3);
        for b in self.hidden.as_bytes() {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 渲染（不泄露隐藏内容）
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "echo:fd={} echo_on={} typed={} echoed={} hidden_len={}",
            self.sess.fd, self.sess.echo_on, self.typed, self.echoed, self.hidden.len()
        )
    }
}

impl Default for EchoDesk {
    fn default() -> Self {
        Self::new(0)
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    // 路1：关闭会话键入 → ECLOSED
    let mut d = EchoDesk::new(1);
    d.sess.open = false;
    let r1 = matches!(d.key('a'), Err(x) if x == EC_ECLOSED);
    // 路2：泄漏面：关回显后伪造回显字节增长必被检出
    let mut d2 = EchoDesk::new(1);
    d2.set_echo(false).ok();
    let before = d2.echoed;
    d2.echoed = d2.echoed.wrapping_add(1); // 模拟泄漏
    let r2 = !d2.no_leak(before);
    // 路3：完整性面：隐藏缓冲与期望不符必被检出
    d2.submit();
    let r3 = !d2.hidden_ok("secret");
    // 路4：恢复面：回显差值账不平必被检出
    let r4 = !d2.echo_ok(5, 3);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮 10 键——关回显段零回显 + 开回显段逐字回显，
/// 两段账全对记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut d = EchoDesk::new(0);
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        // 段1：关回显 5 键
        let _ = d.set_echo(false);
        let eb = d.echoed;
        let mut off_ok = true;
        for k in 0..5u32 {
            let ch = (b'0' + ((r + k) % 10) as u8) as char;
            if d.key(ch) != Ok(false) {
                off_ok = false;
            }
        }
        let off_clean = d.no_leak(eb) && d.echoed == eb;
        // 段2：开回显 5 键
        let _ = d.set_echo(true);
        let es = d.echoed;
        let mut on_ok = true;
        for k in 0..5u32 {
            let ch = (b'A' + ((r + k) % 26) as u8) as char;
            if d.key(ch) != Ok(true) {
                on_ok = false;
            }
        }
        let on_exact = d.echo_ok(es, 5);
        if off_ok && off_clean && on_ok && on_exact {
            good = good.wrapping_add(1);
        }
        // 每 40 轮提交清缓冲，防溢出
        if r % 40 == 39 {
            let _ = d.submit();
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10970_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10970");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "ECLOSED/泄漏/完整性/恢复面坏样本全部命中");

    // J1 关回显零回显 100/100
    let mut d = EchoDesk::new(0);
    let _ = d.set_echo(false);
    let eb = d.echoed;
    let mut zeros = 0u32;
    for i in 0..100u32 {
        let ch = (b'a' + (i % 26) as u8) as char;
        if d.key(ch) == Ok(false) {
            zeros = zeros.wrapping_add(1);
        }
    }
    let j1 = zeros == 100 && d.echoed == eb && d.echo_stream.is_empty();
    s.add(
        "J1-关回显零回显100/100",
        j1,
        "百键键入回显流零字节且逐键返回未回显",
    );

    // 开回显逐字恢复 10/10
    let mut on10 = 0u32;
    for i in 0..10u32 {
        let mut d2 = EchoDesk::new(1);
        let _ = d2.set_echo(false);
        let _ = d2.key_str("pass");
        let _ = d2.set_echo(true);
        let es = d2.echoed;
        let word = crate::alloc::format!("ok{}", i);
        let n = d2.key_str(&word);
        if n == Ok(word.len() as u32) && d2.echo_ok(es, word.len() as u64) {
            on10 = on10.wrapping_add(1);
        }
    }
    s.add("J2-逐字回显恢复10/10", on10 == 10, "ECHO 位恢复后逐字回显十轮全对");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮双段", good == 1000, "千轮关/开回显双段账全对");

    // 隐藏缓冲完整性
    s.add(
        "J4-隐藏缓冲完整性",
        d.hidden.len() == 100 && d.typed == 100,
        "百键全部进入隐藏缓冲",
    );

    // 提交清空
    let secret = d.submit();
    let sub_ok = secret.len() == 100 && d.hidden.is_empty();
    s.add("J5-提交清空", sub_ok, "提交后隐藏缓冲清零且内容完整");

    // 错误码分段
    let codes = [EC_EBADF, EC_EINVAL, EC_ELEAK, EC_EDRIFT, EC_ENOSPC, EC_ECLOSED];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J6-错误码分段", seg_ok, "六码全部落在 0x1d8c0 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J7-错误码互异", distinct, "六码两两互异");

    // 账目对平
    let mut d3 = EchoDesk::new(2);
    let _ = d3.key_str("secret");
    s.add("J8-账目对平", d3.ledger_ok(6) && d3.hidden_ok("secret"), "六键键入与隐藏缓冲一致");

    // 容量面
    let mut d4 = EchoDesk::new(3);
    let mut cap_face = false;
    for i in 0..(HIDDEN_CAP + 5) {
        if d4.key('x') == Err(EC_ENOSPC) {
            cap_face = i == HIDDEN_CAP;
            break;
        }
    }
    s.add("J9-容量面", cap_face, "缓冲满额后的下一键必报 ENOSPC");

    // fold 稳定
    let h1 = d3.fold_all();
    let h2 = d3.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染不泄密
    let r = d3.render();
    s.add(
        "J11-渲染不泄密",
        r.starts_with("echo:") && !r.contains("secret") && r.contains("hidden_len=6"),
        "摘要只报长度不报内容",
    );

    // ECHO 位面
    let mut d5 = EchoDesk::new(4);
    let _ = d5.set_echo(false);
    let off_face = !d5.sess.echo_on && d5.set_echo(true).is_ok() && d5.sess.echo_on;
    s.add("J12-ECHO位面", off_face, "ECHO 位关/开切换正确");

    // 动态判据
    s.add_dyn(
        "J13-键入账",
        d.typed == 100 && d.echoed == 0,
        &crate::alloc::format!("typed={} echoed={}", d.typed, d.echoed),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-回显账",
        d3.echoed == 6 && d3.sess.echo_on,
        &crate::alloc::format!("echoed={} echo_on={}", d3.echoed, d3.sess.echo_on),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10970_echo_control_flow() {
        let mut d = EchoDesk::new(0);
        let _ = d.set_echo(false);
        assert_eq!(d.key_str("hunter2"), Ok(0));
        assert_eq!(d.echo_stream.len(), 0);
        let _ = d.set_echo(true);
        assert_eq!(d.key_str("ab"), Ok(2));
        assert_eq!(d.echo_stream, "ab");
        assert!(d.hidden_ok("hunter2ab"));
        assert_eq!(d.submit(), "hunter2ab");
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10970_checks_green() {
        let s = run_unx10970_checks();
        assert!(s.all_passed(), "UNX-F10970 判据未全绿");
    }
}
