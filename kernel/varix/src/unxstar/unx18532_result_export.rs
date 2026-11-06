//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18532 探针结果导出（AI-87 验收消费格式） · 0x800C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18532-J1 导出格式与 AI-87 验收机器人签名一致，冒烟双绿。
//! 结构清单：
//!   - 数据结构：RowRec（Copy 行记录：编号/裁决/总数/通过/签名）、
//!     ExportSession（64 行数组账本 + green 位面 + 三态导出会话）、
//!     冻结已知答案表（6 条 KAT 签名常量，rustc 独立脚本推导）；
//!   - 状态机：Idle→Open→Sealed 三态，封板后追加显式拒绝；
//!   - 账本：行账（追加序单调）+ 拒因账（只增不减）+ green 位面对账；
//!   - 错误路径：XpCode 闭集 12 码，code() = SEG_BASE + n（0x800C0 段），
//!     每码非空中文建议；
//!   - 注入核：签名篡改 / 字段缺列 / 裁决字非法 / 魔数错 四类坏行
//!     各 10 轮逐类断言被显式拒绝（10/10 全抓）。
//! 签名规格（AI-87 消费端冻结）：seed = 0x87，逐字节 acc = acc*31 + byte
//!   （u32 wrapping），末态截 16 位（& 0xFFFF）。KAT 已知答案：
//!   sig("AI87|100|G|4/4")=0xD1CB、sig("AI87|101|R|4/2")=0x0F95、
//!   sig("AI87|102|G|3/3")=0x448B、sig("AI87|103|R|5/1")=0x89D7、
//!   sig("AI87")=0x0A2E、sig("AI87|104|G|2/2")=0xB74B。
//! 纪律：no_std + alloc；内核路径零 panic；无浮点/IO/墙钟/随机。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x800C0，AI-VX11-1105 G1 批07 独占区，卡表给定）。
pub const SEG_BASE: u32 = 0x800C0;

// ── 冻结常量（推导注明） ─────────────────────────────────────────
/// 导出册容量（64 行：一批探针结果上限，与登记册 128 槽对半配平）。
pub const EXPORT_CAP: usize = 64;
/// AI-87 签名种子（消费端编号 87 → 0x87）。
pub const AI87_SIG_SEED: u32 = 0x87;
/// AI-87 签名位宽（16 位槽：消费端机器人签名寄存器宽度）。
pub const AI87_SIG_MASK: u32 = 0xFFFF;
/// 行格式魔数（AI-87 验收机器人前缀闭集）。
pub const AI87_MAGIC: &str = "AI87";
/// 行字段数（魔数|编号|裁决|总数/通过|签名 = 5 段）。
pub const ROW_FIELDS: usize = 5;
/// 裁决字：绿（全过）。
pub const VERDICT_GREEN: u8 = 1;
/// 裁决字：红（有失败）。
pub const VERDICT_RED: u8 = 2;
/// 探针编号下限（与 F18531 登记册同域）。
pub const PROBE_ID_MIN: u32 = 1;
/// 探针编号上限。
pub const PROBE_ID_MAX: u32 = 4096;
/// 会话状态字：空闲。
pub const SESSION_IDLE: u8 = 1;
/// 会话状态字：开册（可追加）。
pub const SESSION_OPEN: u8 = 2;
/// 会话状态字：封板（只读）。
pub const SESSION_SEALED: u8 = 3;
/// 冒烟行数（8 行：双绿冒烟最小批次）。
pub const SMOKE_ROWS: usize = 8;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类别数（4 类坏行）。
pub const INJECT_CLASSES: usize = 4;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XpCode {
    /// 空行（导出/解析都拒）
    EmptyRow,
    /// 魔数错（前缀非 AI87）
    BadMagic,
    /// 字段缺列（段数 ≠ 5）
    FieldCountWrong,
    /// 裁决字非法（G/R 闭集外）
    BadVerdictChar,
    /// 数值字段解析失败
    BadNumberField,
    /// 签名不符（篡改/错算）
    SigMismatch,
    /// 零判据探针（无账可导）
    ZeroChecks,
    /// 探针编号越界
    ProbeIdOutOfRange,
    /// 导出册满（64 行耗尽）
    LedgerFull,
    /// 封板后追加
    SessionSealedAppend,
    /// 守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl XpCode {
    /// 诊断码取值：SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 非空中文建议（文案闭集）。
    pub fn detail(self) -> &'static str {
        match self {
            XpCode::EmptyRow => "空行：导出/解析均拒，行账不收空笔",
            XpCode::BadMagic => "魔数错：行前缀必须是 AI87，验收机器人只认该前缀",
            XpCode::FieldCountWrong => "字段缺列：行格式为 魔数|编号|裁决|总数/通过|签名 五段",
            XpCode::BadVerdictChar => "裁决字非法：裁决闭集为 G（绿）/ R（红）",
            XpCode::BadNumberField => "数值字段解析失败：编号/判据数/签名必须为十进制数字",
            XpCode::SigMismatch => "签名不符：重算签名与行尾签名不一致，疑似篡改",
            XpCode::ZeroChecks => "零判据探针：无账可导，先补判据再出结果",
            XpCode::ProbeIdOutOfRange => "探针编号越界：编号须在 1..=4096",
            XpCode::LedgerFull => "导出册满：64 行耗尽，封板后另起会话",
            XpCode::SessionSealedAppend => "封板后追加：会话已封板只读，追加显式拒绝",
            XpCode::ConservationBroken => "守恒破坏：行数 = 绿行数 + 红行数 不成立",
            XpCode::NotReproducible => "双跑不一致：同输入导出两次文本不同，复现红线",
        }
    }
    /// 码闭集清单。
    pub const ALL: [XpCode; 12] = [
        XpCode::EmptyRow,
        XpCode::BadMagic,
        XpCode::FieldCountWrong,
        XpCode::BadVerdictChar,
        XpCode::BadNumberField,
        XpCode::SigMismatch,
        XpCode::ZeroChecks,
        XpCode::ProbeIdOutOfRange,
        XpCode::LedgerFull,
        XpCode::SessionSealedAppend,
        XpCode::ConservationBroken,
        XpCode::NotReproducible,
    ];
}

/// 诊断码 → 中文建议（闭集外返回未知提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < XpCode::ALL.len() {
        if XpCode::ALL[i].code() == code {
            return XpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x800C0 段闭集内"
}

// ── AI-87 签名核（规格冻结，KAT 见文件头） ──────────────────────
/// AI-87 签名：seed=0x87，逐字节 acc=acc*31+byte（u32 wrapping），末态截 16 位。
pub fn ai87_signature(content: &[u8]) -> u32 {
    let mut acc: u32 = AI87_SIG_SEED;
    let mut i = 0;
    while i < content.len() {
        match content.get(i) {
            Some(&b) => {
                acc = acc.wrapping_mul(31).wrapping_add(b as u32);
            }
            None => break,
        }
        i += 1;
    }
    acc & AI87_SIG_MASK
}

/// 十进制无符号解析（手写，无 panic；空串/非数字返回 None）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < b.len() {
        let c = match b.get(i) {
            Some(&v) => v,
            None => return None,
        };
        if c < b'0' || c > b'9' {
            return None;
        }
        acc = acc.wrapping_mul(10).wrapping_add((c - b'0') as u32);
        i += 1;
    }
    Some(acc)
}

/// 大写十六进制解析（签名槽用：导出行签名按 {:04X} 落盘）。
pub fn parse_hex_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 8 {
        return None;
    }
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < b.len() {
        let c = match b.get(i) {
            Some(&v) => v,
            None => return None,
        };
        let d = if c >= b'0' && c <= b'9' {
            (c - b'0') as u32
        } else if c >= b'A' && c <= b'F' {
            (c - b'A') as u32 + 10
        } else if c >= b'a' && c <= b'f' {
            (c - b'a') as u32 + 10
        } else {
            return None;
        };
        acc = acc.wrapping_mul(16).wrapping_add(d);
        i += 1;
    }
    Some(acc)
}

// ── 行记录（Copy） ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowRec {
    /// 探针编号。
    pub id: u32,
    /// 裁决（VERDICT_GREEN / VERDICT_RED）。
    pub verdict: u8,
    /// 判据总数。
    pub total: u32,
    /// 判据通过数。
    pub passed: u32,
    /// AI-87 签名（对前四段文本计算）。
    pub sig: u32,
    /// 空槽标记。
    pub empty: bool,
}

impl RowRec {
    /// 空行（const fn：定容数组初始化用）。
    pub const fn empty() -> RowRec {
        RowRec {
            id: 0,
            verdict: 0,
            total: 0,
            passed: 0,
            sig: 0,
            empty: true,
        }
    }
}

/// 单行导出：RowRec → "AI87|<id>|<v>|<total>/<passed>|<sig>"。
/// 签名只盖前四段（AI-87 规格签名域）。
pub fn export_row(rec: &RowRec) -> Result<String, u32> {
    if rec.empty || rec.id == 0 {
        return Err(XpCode::EmptyRow.code());
    }
    let v = match rec.verdict {
        VERDICT_GREEN => "G",
        VERDICT_RED => "R",
        _ => return Err(XpCode::BadVerdictChar.code()),
    };
    // 内容段（签名域）：魔数|编号|裁决|总数/通过。
    let content = format!("{}|{}|{}|{}/{}", AI87_MAGIC, rec.id, v, rec.total, rec.passed);
    let sig = ai87_signature(content.as_bytes());
    Ok(format!("{}|{:04X}", content, sig))
}

/// 单行解析：文本 → RowRec（签名重算强校验，往返可逆）。
pub fn parse_row(line: &str) -> Result<RowRec, u32> {
    if line.is_empty() {
        return Err(XpCode::EmptyRow.code());
    }
    // 五段切分（手写扫描，无 split 依赖）。
    let mut cuts: [usize; ROW_FIELDS + 1] = [0; ROW_FIELDS + 1];
    let mut n = 0usize;
    let bytes = line.as_bytes();
    let mut i = 0;
    cuts[0] = 0;
    while i < bytes.len() {
        if bytes[i] == b'|' {
            n += 1;
            if n >= ROW_FIELDS {
                // 第五段之后不允许再有分隔（多余列即缺列变体）。
                return Err(XpCode::FieldCountWrong.code());
            }
            cuts[n] = i + 1;
        }
        i += 1;
    }
    if n != ROW_FIELDS - 1 {
        return Err(XpCode::FieldCountWrong.code());
    }
    cuts[ROW_FIELDS] = bytes.len();
    // 段抽取（先 let 绑定，避免临时借用悬垂）。
    let f0 = &line[cuts[0]..cuts[1] - 1];
    let f1 = &line[cuts[1]..cuts[2] - 1];
    let f2 = &line[cuts[2]..cuts[3] - 1];
    let f3 = &line[cuts[3]..cuts[4] - 1];
    let f4 = &line[cuts[4]..cuts[5]];
    if f0 != AI87_MAGIC {
        return Err(XpCode::BadMagic.code());
    }
    let id = match parse_u32(f1) {
        Some(v) => v,
        None => return Err(XpCode::BadNumberField.code()),
    };
    let verdict = match f2 {
        "G" => VERDICT_GREEN,
        "R" => VERDICT_RED,
        _ => return Err(XpCode::BadVerdictChar.code()),
    };
    // 总数/通过 复合段。
    let slash = match f3.find('/') {
        Some(p) => p,
        None => return Err(XpCode::FieldCountWrong.code()),
    };
    let total = match parse_u32(&f3[..slash]) {
        Some(v) => v,
        None => return Err(XpCode::BadNumberField.code()),
    };
    let passed = match parse_u32(&f3[slash + 1..]) {
        Some(v) => v,
        None => return Err(XpCode::BadNumberField.code()),
    };
    let sig = match parse_hex_u32(f4) {
        Some(v) => v,
        None => return Err(XpCode::BadNumberField.code()),
    };
    // 语义闸：编号域 / 零判据 / 通过不超总数。
    if id < PROBE_ID_MIN || id > PROBE_ID_MAX {
        return Err(XpCode::ProbeIdOutOfRange.code());
    }
    if total == 0 {
        return Err(XpCode::ZeroChecks.code());
    }
    if passed > total {
        return Err(XpCode::BadNumberField.code());
    }
    // 签名重算强校验（签名域 = 前四段原文）。
    let content = format!("{}|{}|{}|{}/{}", AI87_MAGIC, id, f2, total, passed);
    let want = ai87_signature(content.as_bytes());
    if want != sig {
        return Err(XpCode::SigMismatch.code());
    }
    Ok(RowRec { id, verdict, total, passed, sig, empty: false })
}

// ── 导出会话（三态状态机 + 行账 + green 位面） ──────────────────
pub struct ExportSession {
    /// 64 行数组账本。
    rows: [RowRec; EXPORT_CAP],
    /// green 位面（裁决为绿即置位，对账用）。
    green_plane: [u64; 2],
    /// 已写行数。
    count: usize,
    /// 会话状态（SESSION_* 闭集）。
    state: u8,
    /// 封板后追加被拒笔数（显性记账）。
    pub sealed_append_rejects: u32,
    /// 拒因账（只增不减）。
    reject_ledger: Vec<u32>,
}

impl ExportSession {
    pub fn new() -> ExportSession {
        ExportSession {
            rows: [RowRec::empty(); EXPORT_CAP],
            green_plane: [0u64; 2],
            count: 0,
            state: SESSION_IDLE,
            sealed_append_rejects: 0,
            reject_ledger: Vec::new(),
        }
    }

    /// 开册：Idle → Open。
    pub fn open(&mut self) -> Result<(), u32> {
        if self.state != SESSION_IDLE {
            self.reject_ledger.push(XpCode::SessionSealedAppend.code());
            return Err(XpCode::SessionSealedAppend.code());
        }
        self.state = SESSION_OPEN;
        Ok(())
    }

    /// 追加一行结果（编号/裁决/总数/通过），签名会话内核自动盖。
    pub fn append(&mut self, id: u32, verdict: u8, total: u32, passed: u32) -> Result<u32, u32> {
        // 状态闸先行（封板/未开册均拒）。
        if self.state != SESSION_OPEN {
            self.sealed_append_rejects = self.sealed_append_rejects.saturating_add(1);
            self.reject_ledger.push(XpCode::SessionSealedAppend.code());
            return Err(XpCode::SessionSealedAppend.code());
        }
        // 语义闸（次序：编号→判据→通过≤总数→裁决闭集）。
        if id < PROBE_ID_MIN || id > PROBE_ID_MAX {
            self.reject_ledger.push(XpCode::ProbeIdOutOfRange.code());
            return Err(XpCode::ProbeIdOutOfRange.code());
        }
        if total == 0 {
            self.reject_ledger.push(XpCode::ZeroChecks.code());
            return Err(XpCode::ZeroChecks.code());
        }
        if passed > total {
            self.reject_ledger.push(XpCode::BadNumberField.code());
            return Err(XpCode::BadNumberField.code());
        }
        let v = match verdict {
            VERDICT_GREEN => VERDICT_GREEN,
            VERDICT_RED => VERDICT_RED,
            _ => {
                self.reject_ledger.push(XpCode::BadVerdictChar.code());
                return Err(XpCode::BadVerdictChar.code());
            }
        };
        // 容量闸。
        if self.count >= EXPORT_CAP {
            self.reject_ledger.push(XpCode::LedgerFull.code());
            return Err(XpCode::LedgerFull.code());
        }
        // 造行（先算签名，再可变借用写账）。
        let content = format!(
            "{}|{}|{}|{}/{}",
            AI87_MAGIC,
            id,
            if v == VERDICT_GREEN { "G" } else { "R" },
            total,
            passed
        );
        let sig = ai87_signature(content.as_bytes());
        let idx = self.count;
        let rec = RowRec { id, verdict: v, total, passed, sig, empty: false };
        match self.rows.get_mut(idx) {
            Some(sl) => {
                *sl = rec;
            }
            None => {
                self.reject_ledger.push(XpCode::LedgerFull.code());
                return Err(XpCode::LedgerFull.code());
            }
        }
        if v == VERDICT_GREEN {
            // green 位面置位（idx < 64 → 位面 0）。
            let p = idx / 64;
            let bit = idx % 64;
            if let Some(w) = self.green_plane.get_mut(p) {
                *w |= 1u64 << bit;
            }
        }
        self.count += 1;
        Ok(sig)
    }

    /// 封板：Open → Sealed（终态，只读）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != SESSION_OPEN {
            self.reject_ledger.push(XpCode::SessionSealedAppend.code());
            return Err(XpCode::SessionSealedAppend.code());
        }
        self.state = SESSION_SEALED;
        Ok(())
    }

    /// 全册渲染（追加序即行序，确定性）。
    pub fn render(&self) -> Result<String, u32> {
        let mut out = String::new();
        let mut i = 0;
        while i < self.count {
            let row = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(XpCode::ConservationBroken.code()),
            };
            let line = export_row(&row)?;
            out.push_str(&line);
            out.push('\n');
            i += 1;
        }
        Ok(out)
    }

    /// green 位面对账：popcount == 绿行数（逐位重扫）。
    pub fn audit_green_plane(&self) -> Result<(), u32> {
        let mut greens = 0usize;
        let mut i = 0;
        while i < self.count {
            let row = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(XpCode::ConservationBroken.code()),
            };
            if row.verdict == VERDICT_GREEN {
                greens += 1;
                // 位面必须同位有置位。
                let bit = match self.green_plane.get(i / 64) {
                    Some(w) => (w >> (i % 64)) & 1 == 1,
                    None => false,
                };
                if !bit {
                    return Err(XpCode::ConservationBroken.code());
                }
            }
            i += 1;
        }
        // 位面 popcount 手算。
        let mut pop = 0usize;
        let mut p = 0;
        while p < self.green_plane.len() {
            let mut w = match self.green_plane.get(p) {
                Some(v) => *v,
                None => 0,
            };
            while w != 0 {
                w &= w.wrapping_sub(1);
                pop += 1;
            }
            p += 1;
        }
        if pop != greens {
            return Err(XpCode::ConservationBroken.code());
        }
        Ok(())
    }

    /// 守恒：行数 = 绿 + 红，绿位面无越位。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut greens = 0u32;
        let mut reds = 0u32;
        let mut i = 0;
        while i < self.count {
            let row = match self.rows.get(i) {
                Some(r) => *r,
                None => return Err(XpCode::ConservationBroken.code()),
            };
            if row.verdict == VERDICT_GREEN {
                greens += 1;
            } else {
                reds += 1;
            }
            i += 1;
        }
        if (greens + reds) as usize != self.count {
            return Err(XpCode::ConservationBroken.code());
        }
        self.audit_green_plane()
    }

    pub fn row_count(&self) -> usize {
        self.count
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    pub fn reject_count(&self) -> usize {
        self.reject_ledger.len()
    }
}

// ── 冒烟双绿（J1 直接验收项） ───────────────────────────────────
/// 一次冒烟：8 行全绿探针结果 → 开册追加封板 → 全行回读解析全绿。
/// 返回 (行数, 回读绿行数)。
pub fn smoke_run(round: u32) -> Result<(usize, usize), u32> {
    let mut s = ExportSession::new();
    s.open()?;
    // 8 枚冒烟探针：编号 100..107，判据数 2..9 递变，全绿（passed == total）。
    let mut i = 0;
    while i < SMOKE_ROWS {
        let id = 100 + i as u32;
        let total = 2 + (i as u32 % 8); // 2..9 递变
        let _ = s.append(id, VERDICT_GREEN, total, total)?;
        i += 1;
    }
    s.seal()?;
    s.conserve()?;
    // 回读解析：每行必须签名一致且为绿（round 参数参与确定性注入回放位，
    // 两轮冒烟行内容一致，故 render 必须逐字节相同）。
    let text = s.render()?;
    let mut greens = 0usize;
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut k = 0usize;
    while k <= bytes.len() {
        if k == bytes.len() || bytes[k] == b'\n' {
            if k > start {
                // 行切片（先绑定再解析，避免悬垂临时）。
                let line = &text[start..k];
                let rec = parse_row(line)?;
                if rec.verdict == VERDICT_GREEN {
                    greens += 1;
                }
            }
            start = k + 1;
        }
        k += 1;
    }
    if greens != SMOKE_ROWS {
        return Err(XpCode::ConservationBroken.code());
    }
    if round > 2 {
        return Err(XpCode::ConservationBroken.code());
    }
    Ok((s.row_count(), greens))
}

/// 冒烟双绿：两次独立冒烟全绿且渲染逐字节一致。
pub fn smoke_double_green() -> Result<bool, u32> {
    let a = smoke_run(1)?;
    let b = smoke_run(2)?;
    if a != b {
        return Err(XpCode::NotReproducible.code());
    }
    Ok(a.1 == SMOKE_ROWS && b.1 == SMOKE_ROWS)
}

// ── 黄金链：7 绿 1 红 → 封板 → 往回解析逐行一致 ─────────────────
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let mut s = ExportSession::new();
    s.open()?;
    // 8 行：7 绿 1 红（第 4 行红：4/2，KAT 覆盖裁决 R 与签名 0x0F95）。
    let _ = s.append(100, VERDICT_GREEN, 4, 4)?;
    let _ = s.append(101, VERDICT_RED, 4, 2)?;
    let _ = s.append(102, VERDICT_GREEN, 3, 3)?;
    let _ = s.append(103, VERDICT_RED, 5, 1)?;
    let _ = s.append(104, VERDICT_GREEN, 2, 2)?;
    let _ = s.append(105, VERDICT_GREEN, 6, 6)?;
    let _ = s.append(106, VERDICT_GREEN, 8, 8)?;
    let _ = s.append(107, VERDICT_GREEN, 5, 5)?;
    s.seal()?;
    s.conserve()?;
    let text = s.render()?;
    // 逐行往返：parse → 字段全等。
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut k = 0usize;
    let mut roundtrip = 0usize;
    while k <= bytes.len() {
        if k == bytes.len() || bytes[k] == b'\n' {
            if k > start {
                let line = &text[start..k];
                let rec = parse_row(line)?;
                // 与账内第 roundtrip 行逐字段对拍。
                let src = match s.row_at(roundtrip) {
                    Some(r) => r,
                    None => return Err(XpCode::ConservationBroken.code()),
                };
                if rec != src {
                    return Err(XpCode::NotReproducible.code());
                }
                roundtrip += 1;
            }
            start = k + 1;
        }
        k += 1;
    }
    if roundtrip != GOLDEN_ROWS {
        return Err(XpCode::ConservationBroken.code());
    }
    Ok((s.row_count(), roundtrip, s.reject_count()))
}

/// 黄金链行数（8：6 绿 2 红）。
pub const GOLDEN_ROWS: usize = 8;

impl ExportSession {
    /// 行位只读取值（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<RowRec> {
        match self.rows.get(idx) {
            Some(r) if !r.empty => Some(*r),
            _ => None,
        }
    }
}

// ── KAT 已知答案表（AI-87 签名一致性直接验收） ──────────────────
/// 6 条冻结 KAT：(输入, 期望签名)。值由 rustc 独立脚本按规格推导。
pub const AI87_KAT: [(&str, u32); 6] = [
    ("AI87|100|G|4/4", 0xD1CB),
    ("AI87|101|R|4/2", 0x0F95),
    ("AI87|102|G|3/3", 0x448B),
    ("AI87|103|R|5/1", 0x89D7),
    ("AI87", 0x0A2E),
    ("AI87|104|G|2/2", 0xB74B),
];

/// KAT 全对：每条输入签名与冻结期望一致。
pub fn kat_audit() -> Result<(), u32> {
    let mut i = 0;
    while i < AI87_KAT.len() {
        let (input, want) = AI87_KAT[i];
        let got = ai87_signature(input.as_bytes());
        if got != want {
            return Err(XpCode::SigMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入核：四类坏行，各 10 轮，逐类全抓 ────────────────────────
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn conservation(&self) -> Result<(), u32> {
        let mut tries = 0u32;
        let mut caught = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            tries = tries.saturating_add(*self.tries.get(i).unwrap_or(&0));
            caught = caught.saturating_add(*self.caught.get(i).unwrap_or(&0));
            i += 1;
        }
        if caught > tries {
            return Err(XpCode::ConservationBroken.code());
        }
        Ok(())
    }
}

/// 四类坏行逐类注入 `rounds` 轮：
/// C0 签名篡改、C1 字段缺列、C2 裁决字非法、C3 魔数错。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // C0：真行 + 签名位翻转 → 解析必拒（SigMismatch）。
        let good = export_row(&RowRec {
            id: 100,
            verdict: VERDICT_GREEN,
            total: 4,
            passed: 4,
            sig: 0,
            empty: false,
        })
        .unwrap_or_default();
        // 把行尾签名改成错值（重算后必不符）。
        let cut = match good.rfind('|') {
            Some(p) => p,
            None => 0,
        };
        let mut tampered = String::from(&good[..cut + 1]);
        tampered.push_str("0000");
        led.record(0, parse_row(&tampered).err() == Some(XpCode::SigMismatch.code()));
        // C1：字段缺列（只给三段）。
        led.record(1, parse_row("AI87|100|G").err() == Some(XpCode::FieldCountWrong.code()));
        // C2：裁决字非法。
        led.record(
            2,
            parse_row("AI87|100|X|4/4|1234").err() == Some(XpCode::BadVerdictChar.code()),
        );
        // C3：魔数错。
        led.record(
            3,
            parse_row("AI88|100|G|4/4|1234").err() == Some(XpCode::BadMagic.code()),
        );
        r += 1;
    }
    led.conservation()?;
    Ok(led)
}

/// 容量耗尽：64 行写满后第 65 行必拒且账不丢。
pub fn capacity_probe() -> Result<usize, u32> {
    let mut s = ExportSession::new();
    s.open()?;
    let mut i = 0;
    while i < EXPORT_CAP {
        let _ = s.append(200 + i as u32, VERDICT_GREEN, 3, 3)?;
        i += 1;
    }
    let before = s.row_count();
    let r = s.append(999, VERDICT_GREEN, 3, 3);
    if r.err() != Some(XpCode::LedgerFull.code()) {
        return Err(XpCode::ConservationBroken.code());
    }
    if s.row_count() != before {
        return Err(XpCode::ConservationBroken.code());
    }
    Ok(before)
}

/// 双跑确定性对拍（黄金链两次同值）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(XpCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x80000 || SEG_BASE >= 0x80400 {
        return false;
    }
    let mut i = 0;
    while i < XpCode::ALL.len() {
        let v = XpCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed_set() -> bool {
    let mut i = 0;
    while i < XpCode::ALL.len() {
        if XpCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 判据集（J1 验收） ───────────────────────────────────────────
pub fn run_unx18532_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18532 探针结果导出");
    // 静态明细判据。
    s.add("J1-段位守卫", segment_guard(), "0x800C0 段 0x40 窗口，码闭集不越界");
    s.add(
        "J1-KAT签名一致",
        kat_audit().is_ok(),
        "AI-87 六条已知答案逐条命中（0xD1CB/0x0F95/0x448B/0x89D7/0x0A2E/0xB74B）",
    );
    s.add(
        "J1-封板后追加拒",
        {
            let mut sx = ExportSession::new();
            let _ = sx.open();
            let _ = sx.append(100, VERDICT_GREEN, 3, 3);
            let _ = sx.seal();
            sx.append(101, VERDICT_GREEN, 3, 3).err()
                == Some(XpCode::SessionSealedAppend.code())
        },
        "Sealed 终态只读，追加显式拒绝并记账",
    );
    s.add(
        "J1-边界四类闸",
        {
            let mut sx = ExportSession::new();
            let _ = sx.open();
            let id0 = sx.append(0, VERDICT_GREEN, 3, 3).is_err();
            let zero = sx.append(100, VERDICT_GREEN, 0, 0).is_err();
            let over = sx.append(100, VERDICT_GREEN, 3, 5).is_err();
            let vbad = sx.append(100, 9, 3, 3).is_err();
            id0 && zero && over && vbad
        },
        "编号 0 / 零判据 / 通过>总数 / 裁决越闭集 全拒",
    );
    s.add(
        "J1-守恒绿红齐",
        golden_run().map(|(n, _, _)| n == GOLDEN_ROWS).unwrap_or(false),
        "8 行 = 6 绿 + 2 红，green 位面逐位对账",
    );
    s.add(
        "J1-文案闭集",
        detail_closed_set(),
        "12 码逐码非空中文建议，闭集外落未知提示",
    );
    // 动态明细判据（≤8 条）。
    let smoke1 = smoke_run(1);
    let smoke2 = smoke_run(2);
    let dbl_green = match (smoke1, smoke2) {
        (Ok(a), Ok(b)) => a == b && a.1 == SMOKE_ROWS,
        _ => false,
    };
    s.add_dyn(
        "J1-冒烟双绿",
        dbl_green,
        &format!("冒烟一 = {:?} 冒烟二 = {:?}", smoke_run(1), smoke_run(2)),
    );
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-注入四类10/10全抓",
        inj_ok,
        &format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; INJECT_CLASSES]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; INJECT_CLASSES])
        ),
    );
    let cap = capacity_probe();
    s.add_dyn(
        "J1-容量耗尽账不丢",
        cap.is_ok(),
        &format!("满册行数 = {:?}", cap),
    );
    let rep = repro_check();
    s.add_dyn("J1-双跑一致", rep.is_ok(), &format!("{:?}", rep));
    let rt = golden_run();
    let rt_ok = match rt {
        Ok((_, r, _)) => r == GOLDEN_ROWS,
        Err(_) => false,
    };
    s.add_dyn(
        "J1-往返逐行一致",
        rt_ok,
        &format!("回读对拍行数 = {:?}", golden_run().map(|(_, r, _)| r)),
    );
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kat_signature_constants() {
        assert!(kat_audit().is_ok());
        assert_eq!(ai87_signature(b"AI87|100|G|4/4"), 0xD1CB);
        assert_eq!(ai87_signature(b"AI87"), 0x0A2E);
    }

    #[test]
    fn test_roundtrip_export_parse() {
        assert!(golden_run().is_ok());
        // 单行往返：绿/红各一。
        let g = RowRec { id: 100, verdict: VERDICT_GREEN, total: 4, passed: 4, sig: 0, empty: false };
        let line = export_row(&g).unwrap_or_default();
        let back = parse_row(&line).unwrap_or(RowRec::empty());
        assert_eq!(back.id, 100);
        assert_eq!(back.verdict, VERDICT_GREEN);
        assert_eq!(back.total, 4);
        assert_eq!(back.passed, 4);
    }

    #[test]
    fn test_smoke_double_green() {
        assert!(smoke_double_green().unwrap_or(false));
        let a = smoke_run(1).unwrap_or((0, 0));
        assert_eq!(a.0, SMOKE_ROWS);
        assert_eq!(a.1, SMOKE_ROWS);
    }

    #[test]
    fn test_inject_all_caught_10_rounds() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "四类坏行 10 轮必须全抓");
        assert!(led.conservation().is_ok());
    }

    #[test]
    fn test_session_state_machine() {
        let mut sx = ExportSession::new();
        assert_eq!(sx.state(), SESSION_IDLE);
        assert!(sx.open().is_ok());
        assert_eq!(sx.state(), SESSION_OPEN);
        assert!(sx.append(100, VERDICT_GREEN, 3, 3).is_ok());
        assert!(sx.seal().is_ok());
        assert_eq!(sx.state(), SESSION_SEALED);
        assert!(sx.append(101, VERDICT_GREEN, 3, 3).is_err());
        assert_eq!(sx.sealed_append_rejects, 1);
        // 未开册直接追加也拒。
        let mut sy = ExportSession::new();
        assert!(sy.append(100, VERDICT_GREEN, 3, 3).is_err());
    }

    #[test]
    fn test_capacity_exhaustion() {
        assert_eq!(capacity_probe().unwrap_or(0), EXPORT_CAP);
        let mut sx = ExportSession::new();
        let _ = sx.open();
        let mut i = 0;
        while i < EXPORT_CAP {
            assert!(sx.append(200 + i as u32, VERDICT_GREEN, 3, 3).is_ok());
            i += 1;
        }
        assert!(sx.append(999, VERDICT_GREEN, 3, 3).is_err());
        assert_eq!(sx.row_count(), EXPORT_CAP);
    }

    #[test]
    fn test_boundary_four_classes() {
        let mut sx = ExportSession::new();
        let _ = sx.open();
        assert!(sx.append(0, VERDICT_GREEN, 3, 3).is_err());
        assert!(sx.append(PROBE_ID_MAX + 1, VERDICT_GREEN, 3, 3).is_err());
        assert!(sx.append(100, VERDICT_GREEN, 0, 0).is_err());
        assert!(sx.append(100, VERDICT_GREEN, 3, 5).is_err());
        assert!(sx.append(100, 7, 3, 3).is_err());
    }

    #[test]
    fn test_parse_rejects_bad_rows() {
        assert!(parse_row("").is_err());
        assert!(parse_row("AI87").is_err()); // 纯魔数无列 → 缺列
        assert!(parse_row("AI87|100|G").is_err());
        assert!(parse_row("XX87|100|G|4/4|1234").is_err());
        assert!(parse_row("AI87|abc|G|4/4|1234").is_err());
        assert!(parse_row("AI87|100|Q|4/4|1234").is_err());
        assert!(parse_row("AI87|100|G|4/4|XXXX").is_err());
    }

    #[test]
    fn test_double_run_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_green_plane_audit_and_conserve() {
        let mut sx = ExportSession::new();
        let _ = sx.open();
        let _ = sx.append(100, VERDICT_GREEN, 3, 3);
        let _ = sx.append(101, VERDICT_RED, 3, 1);
        let _ = sx.append(102, VERDICT_GREEN, 3, 3);
        assert!(sx.audit_green_plane().is_ok());
        assert!(sx.conserve().is_ok());
        assert_eq!(sx.row_count(), 3);
    }

    #[test]
    fn test_segment_guard_and_detail_closed_set() {
        assert!(segment_guard());
        assert!(detail_closed_set());
        assert_eq!(XpCode::SigMismatch.code(), SEG_BASE + 5);
    }

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18532_checks();
        assert!(set.all_passed(), "UNX-F18532 判据必须全绿：{}", set.red_items_text());
    }
}
