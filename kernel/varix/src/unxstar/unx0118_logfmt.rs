//! UNX-F0118 · 日志格式规范与解析器 · 诊断段 0x2400
//! 验收锚点：UNX-F0118-J1 规范行「[级别][源] 正文」编解码往返，
//! 畸形行全拒。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}};

pub const SEG_BASE: u16 = 0x2400;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BRACKET: u16 = SEG_BASE + 1; // 括号残缺（无 [ 或无 ]）
const E_LEVEL: u16 = SEG_BASE + 2;   // 级别词非法（五级词外）
const E_SRC: u16 = SEG_BASE + 3;     // 源词非法（核/驱/引 三态外）
const E_BODY: u16 = SEG_BASE + 4;    // 正文空
const E_ORDER: u16 = SEG_BASE + 5;   // 段序乱（正文先于源段）
const E_BAL: u16 = SEG_BASE + 6;     // 解析账不平

pub fn detail(code: u16) -> &'static str {
    match code {
        E_BRACKET => "括号残缺：段标记须成对 [..]",
        E_LEVEL => "级别词非法：DEBUG/INFO/WARN/ERROR/FATAL 五词外",
        E_SRC => "源词非法：KERN/DRV/BOOT 三态外",
        E_BODY => "正文空：两段标记后须有非空正文",
        E_ORDER => "段序乱：级别段在前，源段次之，正文最后",
        E_BAL => "解析账不平：受理数与记账不符",
        _ => "未知诊断码",
    }
}

/// 级别词闭集 ↔ 级别值（与 F0105 对齐）。
pub fn level_from_word(w: &str) -> Result<u8, u16> {
    match w {
        "DEBUG" => Ok(0),
        "INFO" => Ok(1),
        "WARN" => Ok(2),
        "ERROR" => Ok(3),
        "FATAL" => Ok(4),
        _ => Err(E_LEVEL),
    }
}

pub fn word_from_level(l: u8) -> Result<&'static str, u16> {
    match l {
        0 => Ok("DEBUG"),
        1 => Ok("INFO"),
        2 => Ok("WARN"),
        3 => Ok("ERROR"),
        4 => Ok("FATAL"),
        _ => Err(E_LEVEL),
    }
}

/// 源词三态。
pub fn src_from_word(w: &str) -> Result<u8, u16> {
    match w {
        "KERN" => Ok(0),
        "DRV" => Ok(1),
        "BOOT" => Ok(2),
        _ => Err(E_SRC),
    }
}

pub fn word_from_src(s: u8) -> Result<&'static str, u16> {
    match s {
        0 => Ok("KERN"),
        1 => Ok("DRV"),
        2 => Ok("BOOT"),
        _ => Err(E_SRC),
    }
}

/// 规范日志行结构。
#[derive(Clone, Debug, PartialEq)]
pub struct LogRecord {
    pub level: u8,
    pub src: u8,
    pub body: String,
}

/// 编码：结构 → 规范行 "[LEVEL][SRC] body"。
pub fn encode(r: &LogRecord) -> Result<String, u16> {
    let lw = word_from_level(r.level)?;
    let sw = word_from_src(r.src)?;
    if r.body.is_empty() {
        return Err(E_BODY);
    }
    Ok(format!("[{}][{}] {}", lw, sw, r.body))
}

/// 解析：规范行 → 结构。段序/括号/词表/正文逐码校验。
pub fn parse(line: &str) -> Result<LogRecord, u16> {
    // 级别段。
    let rest = match line.strip_prefix('[') {
        Some(r) => r,
        None => return Err(E_BRACKET),
    };
    let (lw, rest) = match rest.split_once(']') {
        Some((a, b)) => (a, b),
        None => return Err(E_BRACKET),
    };
    let level = level_from_word(lw)?;
    // 源段。
    let rest = match rest.strip_prefix('[') {
        Some(r) => r,
        None => return Err(E_BRACKET),
    };
    let (sw, rest) = match rest.split_once(']') {
        Some((a, b)) => (a, b),
        None => return Err(E_BRACKET),
    };
    let src = src_from_word(sw)?;
    // 正文：须以空格起头且非空。
    let body = match rest.strip_prefix(' ') {
        Some(b) if !b.is_empty() => b,
        _ => return Err(E_BODY),
    };
    Ok(LogRecord {
        level,
        src,
        body: body.to_string(),
    })
}

/// 解析账：批量行入账，畸形行计数，受理保真。
pub struct FmtBook {
    pub ok: u64,
    pub bad: u64,
    pub bad_by: [u64; 6],
}

impl FmtBook {
    pub fn new() -> FmtBook {
        FmtBook {
            ok: 0,
            bad: 0,
            bad_by: [0; 6],
        }
    }

    pub fn ingest(&mut self, line: &str) -> Result<LogRecord, u16> {
        match parse(line) {
            Ok(r) => {
                self.ok += 1;
                Ok(r)
            }
            Err(c) => {
                self.bad += 1;
                let idx = match c {
                    E_BRACKET => 0,
                    E_LEVEL => 1,
                    E_SRC => 2,
                    E_BODY => 3,
                    E_ORDER => 4,
                    _ => 5,
                };
                self.bad_by[idx] += 1;
                Err(c)
            }
        }
    }

    pub fn audit(&self, fed: u64) -> Result<(), u16> {
        if self.ok + self.bad != fed {
            return Err(E_BAL);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut b = FmtBook::new();
    let mut fed: u64 = 0;

    // R1：编码→解析往返，五级三源抽样全保真。
    for lv in 0..5u8 {
        for src in 0..3u8 {
            let r = LogRecord {
                level: lv,
                src,
                body: format!("msg-{}-{}", lv, src),
            };
            let line = encode(&r)?;
            fed += 1;
            let back = b.ingest(&line)?;
            if back != r {
                return Err(E_BAL);
            }
        }
    }
    b.audit(fed)?;
    hits += 1;

    // R2：级别词↔值、源词↔值 双向闭集核对。
    for lv in 0..5u8 {
        if level_from_word(word_from_level(lv)?)? != lv {
            return Err(E_LEVEL);
        }
    }
    for src in 0..3u8 {
        if src_from_word(word_from_src(src)?)? != src {
            return Err(E_SRC);
        }
    }
    hits += 1;

    // R3：畸形行直方——括号/级别/源/正文四码各一。
    fed += 1;
    if b.ingest("INFO[KERN] no bracket") != Err(E_BRACKET) {
        return Err(E_BRACKET);
    }
    fed += 1;
    if b.ingest("[TRACE][KERN] bad word") != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    fed += 1;
    if b.ingest("[INFO][GPU] bad src") != Err(E_SRC) {
        return Err(E_SRC);
    }
    fed += 1;
    if b.ingest("[INFO][KERN]") != Err(E_BODY) {
        return Err(E_BODY);
    }
    if b.bad != 4 || b.bad_by[0] != 1 || b.bad_by[1] != 1 || b.bad_by[2] != 1 || b.bad_by[3] != 1 {
        return Err(E_BAL);
    }
    b.audit(fed)?;
    hits += 1;

    // R4：正文含空格与标点保真。
    let r = LogRecord {
        level: 3,
        src: 0,
        body: "page fault at 0xDEAD: code=2".to_string(),
    };
    let line = encode(&r)?;
    fed += 1;
    if b.ingest(&line)? != r {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：级别词大小写敏感（info ≠ INFO）→ E_LEVEL。
    if parse("[info][KERN] x") != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    hits += 1;

    // I2：源段残缺（只有级别段）→ E_BRACKET。
    if parse("[INFO] kern direct") != Err(E_BRACKET) {
        return Err(E_BRACKET);
    }
    hits += 1;

    // I3：正文前缺空格 → E_BODY。
    if parse("[INFO][KERN]nospace") != Err(E_BODY) {
        return Err(E_BODY);
    }
    hits += 1;

    // I4：级别词空段 → E_LEVEL；空行 → E_BRACKET。
    if parse("[][KERN] x") != Err(E_LEVEL) {
        return Err(E_LEVEL);
    }
    if parse("") != Err(E_BRACKET) {
        return Err(E_BRACKET);
    }
    hits += 1;

    // I5：账破注入 → E_BAL。
    if b.audit(fed + 1) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
