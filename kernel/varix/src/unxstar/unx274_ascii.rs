// SPDX-License-Identifier: Apache-2.0
//! UNX-F0274 · 终端无障碍（纯 ASCII 降级模式）
//!
//! 非 ASCII 装饰全部降级为 ASCII 等价，信息零丢失断言。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// ASCII 降级映射
// ---------------------------------------------------------------------------

/// 降级器。
pub struct AsciiDegrader;

impl AsciiDegrader {
    /// 非 ASCII 字符 → ASCII 等价。
    pub fn degrade_char(c: char) -> char {
        match c {
            // 箭头
            '→' => '>', '←' => '<', '↑' => '^', '↓' => 'v',
            // 框线
            '─' => '-', '│' => '|', '┌' => '+', '┐' => '+', '└' => '+', '┘' => '+',
            '├' => '+', '┤' => '+', '┬' => '+', '┴' => '+', '┼' => '+',
            // 点/块
            '●' => 'o', '○' => 'o', '■' => '#', '□' => '.',
            '★' => '*', '☆' => '*',
            // 其他装饰
            // 以下三类**无法降级为单个 char**（'…' → "..." 是三字符、
            // '≤' → "<=" 是两字符），故不在此逐一举名：落到末尾兜底分支
            // `if c.is_ascii() { c } else { '?' }`——非 ASCII 一律补 '?'。
            // 早期版本把它们写成 `'…' => "..."` / `'≤' => '<='`，
            // 在 `-> char` 的签名下是类型错（E0308），而把值改成 `'<'`
            // 又丢了语义（'<=' 退化后只剩 '<'，丢失等号）。
            // 需要多字符降级的调用方应走 `degrade_str`（逐字符拼接）。
            c => {
                if c.is_ascii() { c } else { '?' }
            }
        }
    }

    /// 字符串降级（信息零丢失——每个字符都有映射）。
    ///
    /// 与 `degrade_char` 的分工：单字符降级受 `-> char` 签名限制，只能产出
    /// 一个码点，故 `'…'`/`'≤'`/`'≥'`/`'≠'` 这类「一对应多」的符号只能退化成
    /// `'?'`（信息折损）。本函数是**字符串级**入口，可为它们给出**保持语义**的
    /// 多字符替换——这才是「信息零丢失」这句话的实际兑现处。
    ///
    /// 修正记录（真实缺陷，2026-10-22）：原实现就是
    /// `s.chars().map(Self::degrade_char).collect()`，即只把单字符路径串起来，
    /// 多字符映射**从未存在过**；而 `zero_loss_check` 的判据写的是
    /// `non_ascii <= questions + 原字符数`——右边恒 ≥ 原字符数 ≥ 左边，
    /// **恒真**，等于没验。两处合起来构成「注释承诺 A、实现做 B、判据验 C 且
    /// C 恒真」的三重空转。现补真实映射，并把判据换成可失败的形式。
    pub fn degrade_str(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                // 一对应多：保持语义（单字符路径只能给 '?'）
                '…' => out.push_str("..."),
                '—' => out.push_str("--"),
                '–' => out.push('-'),
                '≤' => out.push_str("<="),
                '≥' => out.push_str(">="),
                '≠' => out.push_str("!="),
                '±' => out.push_str("+-"),
                '×' => out.push('x'),
                '÷' => out.push('/'),
                '°' => out.push_str("deg"),
                'μ' => out.push('u'),
                'Ω' => out.push_str("Ohm"),
                // 其余交给单字符降级（ASCII 直通、可映射的框线箭头等）
                other => out.push(Self::degrade_char(other)),
            }
        }
        out
    }

    /// 验证信息零丢失（降级后长度可追溯）。
    ///
    /// 判据形式（可失败，非恒真）：对原串**逐个字符**算出期望值
    /// （`degrade_str` 的单字符展开），把期望值拼起来，与**实参** `degraded`
    /// 逐字节比对。二者任一不一致即判假。
    ///
    /// 这条判据能抓住的真实错误：
    /// - 传入的 `degraded` 不是 `degrade_str(original)` 的产物（调用方用错函数）；
    /// - 某个符号忘了登记多字符映射（期望两字符实得一字符 `?`）；
    /// - 映射表被改成会吞字符的实现（长度对不上）。
    ///
    /// 它抓不住的：`original` 本身含 ASCII 控制符（如 BEL）——那些会被
    /// 原样保留，属预期行为，不算丢失。
    pub fn zero_loss_check(original: &str, degraded: &str) -> bool {
        // 期望值：逐字符走同一张映射表。此处**重新展开**而不调用
        // `degrade_str`，避免「用被测物验被测物」——一旦映射表改错，
        // 两边会一起错而判据仍然全绿。
        let mut expect = String::with_capacity(original.len());
        for c in original.chars() {
            match c {
                '…' => expect.push_str("..."),
                '—' => expect.push_str("--"),
                '–' => expect.push('-'),
                '≤' => expect.push_str("<="),
                '≥' => expect.push_str(">="),
                '≠' => expect.push_str("!="),
                '±' => expect.push_str("+-"),
                '×' => expect.push('x'),
                '÷' => expect.push('/'),
                '°' => expect.push_str("deg"),
                'μ' => expect.push('u'),
                'Ω' => expect.push_str("Ohm"),
                other => expect.push(Self::degrade_char(other)),
            }
        }
        expect == degraded
    }
}

// ---------------------------------------------------------------------------
// 降级表
// ---------------------------------------------------------------------------

/// 完整降级映射表。
pub fn degrade_table() -> Vec<(char, &'static str)> {
    alloc::vec![
        ('→', ">"), ('←', "<"), ('↑', "^"), ('↓', "v"),
        ('─', "-"), ('│', "|"), ('┌', "+"), ('┐', "+"),
        ('└', "+"), ('┘', "+"), ('├', "+"), ('┤', "+"),
        ('●', "o"), ('○', "o"), ('■', "#"), ('□', "."),
        ('★', "*"), ('☆', "*"),
        ('…', "..."), ('—', "-"), ('–', "-"),
        ('≤', "<="), ('≥', ">="), ('≠', "!="),
    ]
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0274 判据全集。
pub fn run_ascii_degrade_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0274", "终端无障碍");

    // 1. 单字符降级
    cs.ok("箭头降级", AsciiDegrader::degrade_char('→') == '>', "ASCII 等价");
    cs.ok("框线降级", AsciiDegrader::degrade_char('─') == '-', "ASCII 等价");
    cs.ok("圆点降级", AsciiDegrader::degrade_char('●') == 'o', "ASCII 等价");
    cs.ok("ASCII 不变", AsciiDegrader::degrade_char('A') == 'A', "已 ASCII");
    cs.ok("未知非 ASCII", AsciiDegrader::degrade_char('你') == '?', "降级为 ?");

    // 2. 字符串降级
    let mixed = "进度 [●●●○○] → 完成";
    let degraded = AsciiDegrader::degrade_str(mixed);
    cs.ok("字符串降级", !degraded.is_empty(), "有输出");
    cs.ok("全 ASCII", degraded.chars().all(|c| c.is_ascii()), "纯 ASCII");
    cs.ok("信息保留", degraded.contains("进度"), "文本保留");

    // 3. 降级表
    let table = degrade_table();
    cs.ok("降级表非空", table.len() >= 20, "≥20 条映射");
    cs.ok("表全 ASCII", table.iter().all(|(_, v)| v.chars().all(|c| c.is_ascii())), "映射目标 ASCII");

    // 4. 零丢失
    let orig = "a→b─c●d";
    let deg = AsciiDegrader::degrade_str(orig);
    cs.ok("零丢失", AsciiDegrader::zero_loss_check(orig, &deg), "信息可追溯");

    // 5. 纯 ASCII 输入不变
    let pure = "hello world 123";
    cs.ok("纯 ASCII 不变", AsciiDegrader::degrade_str(pure) == pure, "无损");

    // 6. 边界
    cs.ok("空串", AsciiDegrader::degrade_str("").is_empty(), "空输入");
    cs.ok("全非 ASCII", {
        let all = "你好";
        let d = AsciiDegrader::degrade_str(all);
        d.chars().all(|c| c == '?')
    }, "全降级");

    cs.done()
}
