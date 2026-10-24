//! UNX-F0116 · 日志注入防护（格式化安全）· 诊断段 0x23E0
//! 验收锚点：UNX-F0116-J1 控制符/转义序/伪造前缀三类注入全拦，
//! 洗净输出保真。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}};

pub const SEG_BASE: u16 = 0x23E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CTRL: u16 = SEG_BASE + 1;  // 控制字符注入（含换行/回车）
const E_ESC: u16 = SEG_BASE + 2;   // ESC 转义序注入
const E_FAKE: u16 = SEG_BASE + 3;  // 伪造前缀注入（[ERR]/[KERN] 等）
const E_LONG: u16 = SEG_BASE + 4;  // 超长（128 字节上限）
const E_BAL: u16 = SEG_BASE + 5;   // 洗净账不平（进出字数不符语义）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_CTRL => "控制字符注入：日志载荷含 0x00~0x1F（\\t 除外）",
        E_ESC => "转义序注入：载荷含 ESC(0x1B) 引导的 CSI 序",
        E_FAKE => "伪造前缀注入：行首伪装日志元信息",
        E_LONG => "载荷超长：128 字节上限",
        E_BAL => "洗净账不平：净化后长度与记账不符",
        _ => "未知诊断码",
    }
}

pub const PAYLOAD_CAP: usize = 128;

/// 伪造前缀闭集（防冒充日志元信息）。
pub const FAKE_PREFIXES: [&str; 5] = ["[ERR]", "[OK]", "[KERN]", "[DRV]", "[PANIC]"];

/// 载荷体检：逐类检出注入，返回首类违码。
pub fn inspect(payload: &str) -> Result<(), u16> {
    if payload.len() > PAYLOAD_CAP {
        return Err(E_LONG);
    }
    let b = payload.as_bytes();
    for (i, c) in b.iter().enumerate() {
        if *c == 0x1B {
            return Err(E_ESC);
        }
        if *c < 0x20 && *c != b'\t' {
            // 伪造前缀判断优先于一般控制符（行首元信息伪装）。
            let rest = &payload[i..];
            for p in FAKE_PREFIXES {
                if rest.starts_with(p) {
                    return Err(E_FAKE);
                }
            }
            return Err(E_CTRL);
        }
    }
    // 行首伪造前缀（无控制符引导的直冒）。
    for p in FAKE_PREFIXES {
        if payload.starts_with(p) {
            return Err(E_FAKE);
        }
    }
    Ok(())
}

/// 洗净器：检出即拦；合法载荷原样放行并记账。
pub struct WashBook {
    pub passed: u64,
    pub blocked: u64,
    pub by_code: [u64; 6], // E_CTRL..E_BAL 六码直方
}

impl WashBook {
    pub fn new() -> WashBook {
        WashBook {
            passed: 0,
            blocked: 0,
            by_code: [0; 6],
        }
    }

    /// 单载荷处理：合法→放行；注入→拦下并按码记账。
    pub fn wash(&mut self, payload: &str) -> Result<String, u16> {
        match inspect(payload) {
            Ok(()) => {
                self.passed += 1;
                Ok(payload.to_string())
            }
            Err(code) => {
                self.blocked += 1;
                let idx = match code {
                    E_CTRL => 0,
                    E_ESC => 1,
                    E_FAKE => 2,
                    E_LONG => 3,
                    E_BAL => 4,
                    _ => 5,
                };
                self.by_code[idx] += 1;
                Err(code)
            }
        }
    }

    /// 账守恒：passed + blocked == 总喂入。
    pub fn audit(&self, fed: u64) -> Result<(), u16> {
        if self.passed + self.blocked != fed {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 拦截直方渲染。
    pub fn render(&self) -> String {
        format!(
            "洗净账：放行 {} 拦截 {}（控{} 转义{} 伪造{} 超长{}）",
            self.passed,
            self.blocked,
            self.by_code[0],
            self.by_code[1],
            self.by_code[2],
            self.by_code[3]
        )
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut w = WashBook::new();
    let mut fed: u64 = 0;

    // R1：合法载荷放行保真（含制表符与中文）。
    for s in ["boot ok", "heap\taligned", "驱动加载完毕"] {
        fed += 1;
        if w.wash(s)? != s {
            return Err(E_BAL);
        }
    }
    w.audit(fed)?;
    hits += 1;

    // R2：三类注入逐码拦截。
    fed += 1;
    if w.wash("line1\nline2") != Err(E_CTRL) {
        return Err(E_CTRL);
    }
    fed += 1;
    if w.wash("bad\x1b[2Jpoison") != Err(E_ESC) {
        return Err(E_ESC);
    }
    fed += 1;
    if w.wash("[ERR] not really") != Err(E_FAKE) {
        return Err(E_FAKE);
    }
    if w.by_code[0] != 1 || w.by_code[1] != 1 || w.by_code[2] != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：超长注入（129 字节）→ E_LONG；直方渲染账。
    let long = "x".repeat(PAYLOAD_CAP + 1);
    fed += 1;
    if w.wash(&long) != Err(E_LONG) {
        return Err(E_LONG);
    }
    let rep = w.render();
    if !rep.contains("放行 3 拦截 4") || !rep.contains("超长1") {
        return Err(E_BAL);
    }
    w.audit(fed)?;
    hits += 1;

    // R4：合法边界——恰 128 字节放行；换行在行中但带伪造前缀变体。
    let edge = "y".repeat(PAYLOAD_CAP);
    fed += 1;
    w.wash(&edge)?;
    w.audit(fed)?;
    hits += 1;

    // I1：回车注入（\r）→ E_CTRL。
    if inspect("a\rb") != Err(E_CTRL) {
        return Err(E_CTRL);
    }
    hits += 1;

    // I2：全部伪造前缀闭集直冒 → E_FAKE。
    for p in FAKE_PREFIXES {
        if inspect(p) != Err(E_FAKE) {
            return Err(E_FAKE);
        }
    }
    hits += 1;

    // I3：控制符引导的伪造前缀 → E_FAKE（优先于 E_CTRL）。
    if inspect("\n[OK] sneak") != Err(E_FAKE) {
        return Err(E_FAKE);
    }
    hits += 1;

    // I4：ESC 在载荷末尾（残缺转义序）→ E_ESC。
    let mut tail_esc = String::from("safe");
    tail_esc.push(0x1B as char);
    if inspect(&tail_esc) != Err(E_ESC) {
        return Err(E_ESC);
    }
    hits += 1;

    // I5：账破注入 → E_BAL。
    let w5 = WashBook {
        passed: 2,
        blocked: 1,
        by_code: [0; 6],
    };
    if w5.audit(2) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
