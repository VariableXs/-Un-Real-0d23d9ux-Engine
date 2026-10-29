//! UNX-F0746 · 域经格式规范冻结（全域统一模板定版） · 诊断段 0x5c40
//! 判据原文摘录：UNX-F0746-J1 规范冻结账齐（模板五字段/粒度/证据三规范 +
//! 版本号 + 哈希封存）且入簿条目符合度 100%；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 模板五字段写死：编号 / 主题 / 粒度 / 证据指针 / 来源批，
//!    字段名、字段序、字段类型三要素入规范体，冻结后不可扩。
//! 2. 粒度三规范：条目粒度限"单主题单结论"，主题 4..=24 字节，
//!    结论 8..=64 字节，粒度越粗（多主题混写）即不合规。
//! 3. 证据三规范：每条 ≥1 证据指针、指针必须可解引、证据块非空字节，
//!    三条全过才算证据合规。
//! 4. 版本号 + 哈希封存：规范体定版为 v1.0.0，规范哈希 = 五字段声明 + 粒度
//!    参数 + 证据参数的确定性聚合；入簿条目符合度必须 100% 才可封存。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5c40 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5c40;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_FIELD_BAD: u16 = SEG_BASE + 1; // 模板字段声明非法（名空/类型越界）
const E_FIELD_FIXED: u16 = SEG_BASE + 2; // 冻结后模板字段不可变更
const E_TOPIC_LEN: u16 = SEG_BASE + 3; // 主题长度越界（4..=24）
pub const _UNUSED_TOPIC: u16 = SEG_BASE + 4; // 预留位（保持判别式连续）
const E_CONCL_LEN: u16 = SEG_BASE + 5; // 结论长度越界（8..=64）
const E_MULTI_TOPIC: u16 = SEG_BASE + 6; // 粒度违规：条目混写多主题
const E_EVID_LACK: u16 = SEG_BASE + 7; // 证据缺失（零指针）
const E_EVID_DANGLE: u16 = SEG_BASE + 8; // 证据指针不可解引
const E_EVID_EMPTY: u16 = SEG_BASE + 9; // 证据块空字节
const E_BATCH_FMT: u16 = SEG_BASE + 10; // 来源批号格式非法（非 Bxx）
const E_ENTRY_FULL: u16 = SEG_BASE + 11; // 入簿账已满
const E_ENTRY_DUP: u16 = SEG_BASE + 12; // 条目重复入簿
const E_COMPLIANCE: u16 = SEG_BASE + 13; // 入簿条目符合度 < 100%
const E_SEALED: u16 = SEG_BASE + 14; // 规范已封存，拒绝再写
const E_NOT_FROZEN: u16 = SEG_BASE + 15; // 规范未冻结即入簿
const E_VER_BAD: u16 = SEG_BASE + 16; // 版本号非法（非 vMAJOR.MINOR.PATCH）
const E_EVID_FULL: u16 = SEG_BASE + 17; // 证据账已满

/// 入簿账定容。
pub const MAX_ENTRIES: usize = 24;

/// 证据块账定容。
pub const MAX_EVID: usize = 48;

/// 主题长度规范：4..=24 字节。
pub const TOPIC_MIN: usize = 4;
pub const TOPIC_MAX: usize = 24;

/// 结论长度规范：8..=64 字节。
pub const CONCL_MIN: usize = 8;
pub const CONCL_MAX: usize = 64;

/// 规范定版版本号。
pub const SPEC_VERSION: &str = "v1.0.0";

// ── FNV-1a 64（规范哈希与封存口径）──────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_FIELD_BAD => "字段声明非法：字段名非空、类型码在闭集",
        E_FIELD_FIXED => "模板冻结后不可变更：变更须出 v2 规范另立新账",
        E_TOPIC_LEN => "主题长度越界：规范 4..=24 字节",
        _ if code == SEG_BASE + 4 => "预留诊断位",
        E_CONCL_LEN => "结论长度越界：规范 8..=64 字节",
        E_MULTI_TOPIC => "粒度违规：一条只许单主题单结论，拆条再入簿",
        E_EVID_LACK => "证据缺失：每条至少挂一个证据指针",
        E_EVID_DANGLE => "证据指针不可解引：先入证据块再入簿",
        E_EVID_EMPTY => "证据块空字节：空证据无效",
        E_BATCH_FMT => "批号格式非法：须形如 Bxx",
        E_ENTRY_FULL => "入簿账已满 24 条：先归档再入",
        E_ENTRY_DUP => "条目重复入簿：主题唯一键",
        E_COMPLIANCE => "符合度不足 100%：逐条按五字段排查",
        E_SEALED => "规范已封存：封存后只读",
        E_NOT_FROZEN => "规范未冻结：先 freeze 再入簿",
        E_VER_BAD => "版本号非法：vMAJOR.MINOR.PATCH 三段式",
        E_EVID_FULL => "证据账已满 48 块：先入簿再补证据",
        _ => "未知诊断码",
    }
}

// ── 版本号校验 ───────────────────────────────────────────────
/// 版本号合法性：v + 三段点分十进制。
pub fn version_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 6 || b[0] != b'v' {
        return false;
    }
    // 结构：v <num>.<num>.<num>。
    let mut segs = 0u32;
    let mut i = 1usize;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            i += 1;
        } else if c == b'.' {
            // 段不得为空：前一个字符必须是数字。
            if i == 1 || !b[i - 1].is_ascii_digit() {
                return false;
            }
            segs += 1;
            i += 1;
        } else {
            return false;
        }
    }
    segs == 2 && b[b.len() - 1].is_ascii_digit()
}

// ── 模板字段 ─────────────────────────────────────────────────
/// 模板字段类型码闭集。
pub const FT_ID: u8 = 1; // 编号
pub const FT_TOPIC: u8 = 2; // 主题
pub const FT_GRAN: u8 = 3; // 粒度
pub const FT_EVID: u8 = 4; // 证据指针
pub const FT_BATCH: u8 = 5; // 来源批

/// 模板字段声明行。
#[derive(Clone, Debug)]
pub struct TemplateField {
    /// 字段名。
    pub name: String,
    /// 字段类型码（闭集 1..=5）。
    pub ty: u8,
    /// 是否必填（规范五字段全部必填）。
    pub required: bool,
}

// ── 规范体（冻结对象）────────────────────────────────────────
/// 域经格式规范体：五字段声明 + 粒度参数 + 证据参数 + 版本号。
pub struct FormatSpec {
    /// 模板五字段（定容 5）。
    fields: [Option<TemplateField>; 5],
    /// 字段数。
    field_count: usize,
    /// 主题长度下限/上限。
    topic_min: usize,
    topic_max: usize,
    /// 结论长度下限/上限。
    concl_min: usize,
    concl_max: usize,
    /// 每条最少证据指针数。
    evid_min: usize,
    /// 版本号。
    version: String,
    /// 规范哈希（冻结后落）。
    spec_hash: u64,
    /// 是否冻结。
    frozen: bool,
}

impl FormatSpec {
    /// 建规范：五字段按定版序装入。
    pub fn new() -> Result<FormatSpec, u16> {
        let mut sp = FormatSpec {
            fields: [None, None, None, None, None],
            field_count: 0,
            topic_min: TOPIC_MIN,
            topic_max: TOPIC_MAX,
            concl_min: CONCL_MIN,
            concl_max: CONCL_MAX,
            evid_min: 1,
            version: String::new(),
            spec_hash: 0,
            frozen: false,
        };
        sp.add_field("编号", FT_ID)?;
        sp.add_field("主题", FT_TOPIC)?;
        sp.add_field("粒度", FT_GRAN)?;
        sp.add_field("证据指针", FT_EVID)?;
        sp.add_field("来源批", FT_BATCH)?;
        Ok(sp)
    }

    /// 装字段：名非空 + 类型闭集 + 五字段定容。
    fn add_field(&mut self, name: &str, ty: u8) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FIELD_FIXED);
        }
        if name.is_empty() || name.len() > 16 {
            return Err(E_FIELD_BAD);
        }
        if ty == 0 || ty > FT_BATCH {
            return Err(E_FIELD_BAD);
        }
        if self.field_count >= 5 {
            return Err(E_FIELD_BAD);
        }
        self.fields[self.field_count] = Some(TemplateField {
            name: name.to_string(),
            ty,
            required: true,
        });
        self.field_count += 1;
        Ok(())
    }

    /// 设版本号并冻结：版本合法 + 五字段齐 → 规范哈希落账。
    pub fn freeze(&mut self, version: &str) -> Result<u64, u16> {
        if self.frozen {
            return Ok(self.spec_hash);
        }
        if !version_ok(version) {
            return Err(E_VER_BAD);
        }
        if self.field_count != 5 {
            return Err(E_FIELD_BAD);
        }
        self.version = version.to_string();
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(version.as_bytes());
        buf.push(0x1f);
        let mut i = 0usize;
        while i < 5 {
            match &self.fields[i] {
                Some(f) => {
                    buf.extend_from_slice(f.name.as_bytes());
                    buf.push(f.ty);
                    buf.push(f.required as u8);
                }
                None => return Err(E_FIELD_BAD),
            }
            i += 1;
        }
        buf.extend_from_slice((self.topic_min as u32).to_le_bytes().as_slice());
        buf.extend_from_slice((self.topic_max as u32).to_le_bytes().as_slice());
        buf.extend_from_slice((self.concl_min as u32).to_le_bytes().as_slice());
        buf.extend_from_slice((self.concl_max as u32).to_le_bytes().as_slice());
        buf.push(self.evid_min as u8);
        self.spec_hash = fnv1a64(&buf);
        self.frozen = true;
        Ok(self.spec_hash)
    }

    /// 规范哈希。
    pub fn hash(&self) -> u64 {
        self.spec_hash
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 版本号。
    pub fn version(&self) -> &str {
        &self.version
    }

    /// 五字段名快照。
    pub fn field_names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.field_count {
            if let Some(f) = &self.fields[i] {
                out.push(f.name.clone());
            }
            i += 1;
        }
        out
    }
}

// ── 证据块 ───────────────────────────────────────────────────
/// 证据块：可解引证据单元。
#[derive(Clone, Debug)]
pub struct SpecEvidence {
    /// 块编号（1 起连续）。
    pub block_id: u32,
    /// 证据字节。
    pub bytes: Vec<u8>,
    /// 块哈希。
    pub hash: u64,
}

// ── 入簿条目 ─────────────────────────────────────────────────
/// 按模板五字段登记的入簿条目。
#[derive(Clone, Debug)]
pub struct SpecEntry {
    /// 字段一：编号（登记序）。
    pub entry_no: u32,
    /// 字段二：主题。
    pub topic: String,
    /// 字段三：粒度（单主题标记：结论字节长）。
    pub conclusion: String,
    /// 字段四：证据指针集。
    pub evids: Vec<u32>,
    /// 字段五：来源批号。
    pub source_batch: String,
}

impl SpecEntry {
    /// 批号合法性（Bxx）。
    fn batch_ok(s: &str) -> bool {
        let b = s.as_bytes();
        if b.len() != 3 || b[0] != b'B' {
            return false;
        }
        let mut i = 1usize;
        while i < 3 {
            if !b[i].is_ascii_digit() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 粒度合规：单主题标记（结论里不含主题分隔符 ';'/'\n'）+ 长度规范。
    fn gran_ok(&self) -> bool {
        let cb = self.conclusion.as_bytes();
        if cb.len() < CONCL_MIN || cb.len() > CONCL_MAX {
            return false;
        }
        let mut i = 0usize;
        while i < cb.len() {
            if cb[i] == b';' || cb[i] == b'\n' {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 主题合规。
    fn topic_ok(&self) -> bool {
        let tb = self.topic.as_bytes();
        tb.len() >= TOPIC_MIN && tb.len() <= TOPIC_MAX
    }

    /// 证据三规范之指针侧：指针数 ≥1。
    fn evid_count_ok(&self) -> bool {
        self.evids.len() >= 1
    }

    /// 五字段静态合规（不含可解引侧）。
    pub fn static_ok(&self) -> bool {
        self.entry_no > 0
            && self.topic_ok()
            && self.gran_ok()
            && self.evid_count_ok()
            && Self::batch_ok(&self.source_batch)
    }
}

// ── 规范账（主体）────────────────────────────────────────────
/// 域经格式规范账：规范冻结 → 入簿 → 符合度 100% → 封存。
pub struct SpecLedger {
    /// 规范体。
    spec: FormatSpec,
    /// 证据块账。
    evids: [Option<SpecEvidence>; MAX_EVID],
    evid_count: usize,
    /// 入簿账。
    entries: [Option<SpecEntry>; MAX_ENTRIES],
    entry_count: usize,
    /// 是否封存。
    sealed: bool,
}

impl SpecLedger {
    /// 开账：规范五字段装入未冻结。
    pub fn new() -> Result<SpecLedger, u16> {
        Ok(SpecLedger {
            spec: FormatSpec::new()?,
            evids: core::array::from_fn(|_| None),
            evid_count: 0,
            entries: core::array::from_fn(|_| None),
            entry_count: 0,
            sealed: false,
        })
    }

    /// 规范冻结（版本号 v1.0.0 定版）。
    pub fn freeze_spec(&mut self, version: &str) -> Result<u64, u16> {
        self.spec.freeze(version)
    }

    /// 规范哈希。
    pub fn spec_hash(&self) -> u64 {
        self.spec.hash()
    }

    /// 入证据块。
    pub fn put_evidence(&mut self, bytes: &[u8]) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.evid_count >= MAX_EVID {
            return Err(E_EVID_FULL);
        }
        if bytes.is_empty() {
            return Err(E_EVID_EMPTY);
        }
        let block_id = (self.evid_count + 1) as u32;
        self.evids[self.evid_count] = Some(SpecEvidence {
            block_id,
            bytes: Vec::from(bytes),
            hash: fnv1a64(bytes),
        });
        self.evid_count += 1;
        Ok(block_id)
    }

    /// 证据解引。
    pub fn deref_evidence(&self, block_id: u32) -> Result<&SpecEvidence, u16> {
        if block_id == 0 || block_id as usize > self.evid_count {
            return Err(E_EVID_DANGLE);
        }
        match &self.evids[(block_id - 1) as usize] {
            Some(b) if !b.bytes.is_empty() => Ok(b),
            _ => Err(E_EVID_EMPTY),
        }
    }

    /// 入簿：规范必须冻结 + 五字段静态合规 + 证据可解引。
    pub fn admit(
        &mut self,
        topic: &str,
        conclusion: &str,
        evids: &[u32],
        source_batch: &str,
    ) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.spec.is_frozen() {
            return Err(E_NOT_FROZEN);
        }
        if self.entry_count >= MAX_ENTRIES {
            return Err(E_ENTRY_FULL);
        }
        // 主题查重。
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                if e.topic == topic {
                    return Err(E_ENTRY_DUP);
                }
            }
            i += 1;
        }
        if topic.as_bytes().len() < TOPIC_MIN || topic.as_bytes().len() > TOPIC_MAX {
            return Err(E_TOPIC_LEN);
        }
        if conclusion.as_bytes().len() < CONCL_MIN
            || conclusion.as_bytes().len() > CONCL_MAX
        {
            return Err(E_CONCL_LEN);
        }
        if evids.is_empty() {
            return Err(E_EVID_LACK);
        }
        if source_batch.as_bytes().len() != 3
            || !source_batch.as_bytes().starts_with(b"B")
        {
            return Err(E_BATCH_FMT);
        }
        // 证据可解引逐条核。
        i = 0;
        while i < evids.len() {
            self.deref_evidence(evids[i])?;
            i += 1;
        }
        let entry_no = (self.entry_count + 1) as u32;
        self.entries[self.entry_count] = Some(SpecEntry {
            entry_no,
            topic: topic.to_string(),
            conclusion: conclusion.to_string(),
            evids: Vec::from(evids),
            source_batch: source_batch.to_string(),
        });
        self.entry_count += 1;
        Ok(entry_no)
    }

    /// 单条合规核：静态五字段 + 证据三规范（可解引 + 非空）。
    fn entry_compliant(&self, e: &SpecEntry) -> bool {
        if !e.static_ok() {
            return false;
        }
        let mut i = 0usize;
        while i < e.evids.len() {
            match self.deref_evidence(e.evids[i]) {
                Ok(b) if !b.bytes.is_empty() => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 符合度核算：合规条数 / 入簿条数（千分整数化，1000 = 100%）。
    pub fn compliance_permille(&self) -> Result<u32, u16> {
        if self.entry_count == 0 {
            return Err(E_COMPLIANCE);
        }
        let mut ok = 0u32;
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                if self.entry_compliant(e) {
                    ok += 1;
                }
            }
            i += 1;
        }
        Ok(ok * 1000 / self.entry_count as u32)
    }

    /// 符合度 100% 断言。
    pub fn compliance_assert(&self) -> Result<(), u16> {
        if self.compliance_permille()? != 1000 {
            return Err(E_COMPLIANCE);
        }
        Ok(())
    }

    /// 入簿条数。
    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    /// 封存：规范冻结 + 符合度 100% → 规范哈希 + 条目聚合封存哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.spec.is_frozen() {
            return Err(E_NOT_FROZEN);
        }
        self.compliance_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.spec_hash().to_le_bytes());
        let mut i = 0usize;
        while i < self.entry_count {
            if let Some(e) = &self.entries[i] {
                buf.extend_from_slice(e.entry_no.to_le_bytes().as_slice());
                buf.extend_from_slice(e.topic.as_bytes());
                buf.push(0x1f);
                buf.extend_from_slice(e.conclusion.as_bytes());
                buf.extend_from_slice(e.source_batch.as_bytes());
                buf.extend_from_slice((e.evids.len() as u32).to_le_bytes().as_slice());
            }
            i += 1;
        }
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 规范声明行渲染。
    pub fn spec_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 域经格式规范 · 全域统一模板 ===".to_string());
        lines.push("版本: ".to_string() + self.spec.version());
        let names = self.spec.field_names();
        let mut i = 0usize;
        while i < names.len() {
            lines.push("字段".to_string() + &itoa((i + 1) as u64) + ": " + &names[i]);
            i += 1;
        }
        lines.push("主题长: 4..=24 字节".to_string());
        lines.push("结论长: 8..=64 字节".to_string());
        lines.push("证据下限: 1 指针/条".to_string());
        lines
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：规范冻结 + 入簿符合度 100% + 封存 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut lg = SpecLedger::new()?;
    let sh = lg.freeze_spec(SPEC_VERSION)?;
    if sh == 0 || !lg.spec.is_frozen() {
        return Err(SEG_BASE);
    }
    // 证据块。
    let ev1 = lg.put_evidence(b"template: five-field row spec")?;
    let ev2 = lg.put_evidence(b"granularity: single topic per entry")?;
    let ev3 = lg.put_evidence(b"evidence: deref + non-empty")?;
    // 入簿三条全合规。
    let e1 = lg.admit("五字段模板", "all rows carry five fields verbatim", &[ev1], "B37")?;
    let e2 = lg.admit("单主题粒度", "one topic one conclusion per entry", &[ev2], "B37")?;
    let e3 = lg.admit("证据三规范", "deref-able and non-empty evidence", &[ev3, ev1], "B37")?;
    if lg.entry_count() != 3 {
        return Err(SEG_BASE);
    }
    let _ = (e1, e2, e3);
    lg.compliance_assert()?;
    if lg.compliance_permille()? != 1000 {
        return Err(SEG_BASE);
    }
    let sealed = lg.seal()?;
    if sealed == 0 {
        return Err(SEG_BASE);
    }
    // 规范声明行可渲染。
    let sl = lg.spec_lines();
    if sl.len() < 8 {
        return Err(SEG_BASE);
    }
    // 封存后入簿拒绝。
    match lg.admit("封存后条目", "should be rejected outright", &[ev1], "B37") {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：未冻结即入簿。
    let mut lg2 = SpecLedger::new()?;
    let ev = lg2.put_evidence(b"evidence first")?;
    match lg2.admit("未冻结条目", "admission before freeze", &[ev], "B37") {
        Err(c) if c == E_NOT_FROZEN => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：坏版本号冻结拒绝。
    let mut lg3 = SpecLedger::new()?;
    match lg3.freeze_spec("1.0") {
        Err(c) if c == E_VER_BAD => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：结论过短（粒度违规）。
    let mut lg4 = SpecLedger::new()?;
    let _ = lg4.freeze_spec(SPEC_VERSION)?;
    let ev4 = lg4.put_evidence(b"ev")?;
    match lg4.admit("短结论条目", "short", &[ev4], "B37") {
        Err(c) if c == E_CONCL_LEN => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
