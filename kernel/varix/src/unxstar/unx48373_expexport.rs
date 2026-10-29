//! unx48373_expexport — UNX-F48373 B19 批导出面（JSON 导出对接总日志中心）（诊断段 0xCF80）。
//!
//! 判据原文：
//!   J1 导出 ×10 schema 校验全过；判据锚定 体验十三·补 总日志中心。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 导出面唯一出口：总日志中心对外的唯一形态是 JSON 导出流；每条
//!      体验事件先过 schema 闸（必填齐全 / 类型相符 / 取值在域 / 未知
//!      字段拒收）再入导出批，未过闸记录一律拒收并报诊断码，绝不带病出站。
//!   2. ×10 全过：本批导出恰 10 条记录，逐条走完整 schema 校验链，
//!      十条全过才计命中——不允许抽样放行，也不允许静默跳过。
//!   3. 序列化确定性：同一记录两次序列化产出逐 token 一致的导出流
//!      （FNV 指纹相等）；导出流末位附整流校验 token，反解时先验
//!      校验 token 再还原字段，任一位被篡改即拒收。
//!   4. 反解全等：导出流反解回的记录必须与原记录字段级全等
//!      （seq/ts/kind/payload 逐位比对），否则导出面不配入账。
//!   5. 总日志中心账目：导出批 append-only，批内 seq 严格递增无重号、
//!      ts 严格单调；重复导出同一批幂等不重记；越序记录拒收。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCF80;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SCHEMA_TYPE: u16 = SEG_BASE + 1;  // 字段类型不符
const E_SCHEMA_FIELD: u16 = SEG_BASE + 2; // 必填缺失/未知字段
const E_SCHEMA_VALUE: u16 = SEG_BASE + 3; // 取值越域
const E_SEQ_ORDER: u16 = SEG_BASE + 4;    // seq 重号/非递增
const E_TS_ORDER: u16 = SEG_BASE + 5;     // ts 非单调
const E_TOKEN_BAD: u16 = SEG_BASE + 6;    // 导出流 token 非法/校验漂移
const E_ROUNDTRIP: u16 = SEG_BASE + 7;    // 反解字段不等
const E_LEDGER_BAD: u16 = SEG_BASE + 8;   // 中心账目违规（条数/幂等）
const E_CODE_RANGE: u16 = SEG_BASE + 9;   // 诊断码越段

/// 导出批条数（判据 ×10）。
pub const EXPORT_N: u32 = 10;

/// schema 字段标签（整数域，导出面禁字符串）。
pub const F_SEQ: u8 = 1;
pub const F_TS: u8 = 2;
pub const F_KIND: u8 = 3;
pub const F_PAYLOAD: u8 = 4;

/// 导出流魔数 token（首 token）。
pub const TOK_MAGIC: u64 = 0x4A_5F_45_58_50_00_00_01;

/// JSON 值域：导出面只放行三类标量。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Num(u64),
}

/// 导出记录对象：有序字段表（tag, value）。
#[derive(Clone, PartialEq, Eq)]
pub struct JsonObj {
    fields: Vec<(u8, JsonValue)>,
}

impl JsonObj {
    pub fn new() -> Self {
        JsonObj { fields: Vec::new() }
    }

    /// 同 tag 覆盖写（保持首插位置），未插过则追加。
    pub fn set(&mut self, tag: u8, v: JsonValue) {
        let mut i = 0;
        while i < self.fields.len() {
            if self.fields[i].0 == tag {
                self.fields[i].1 = v;
                return;
            }
            i += 1;
        }
        self.fields.push((tag, v));
    }

    pub fn get(&self, tag: u8) -> Option<JsonValue> {
        let mut i = 0;
        while i < self.fields.len() {
            if self.fields[i].0 == tag {
                return Some(self.fields[i].1);
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }
}

/// schema 闸：必填 F_SEQ/F_TS/F_KIND 皆为 Num；F_PAYLOAD 可选且须 Num；
/// 未知 tag 拒收；seq ∈ 1..；kind ∈ 1..=4。返回 (seq, ts, kind, payload)。
pub fn schema_check(o: &JsonObj) -> Result<(u64, u64, u8, u64), u16> {
    // 未知字段先扫一遍，直接拒收。
    let mut i = 0;
    while i < o.fields.len() {
        let tag = o.fields[i].0;
        if tag != F_SEQ && tag != F_TS && tag != F_KIND && tag != F_PAYLOAD {
            return Err(E_SCHEMA_FIELD);
        }
        i += 1;
    }
    let seq_v = match o.get(F_SEQ) {
        Some(JsonValue::Num(n)) => n,
        Some(_) => return Err(E_SCHEMA_TYPE),
        None => return Err(E_SCHEMA_FIELD),
    };
    let ts_v = match o.get(F_TS) {
        Some(JsonValue::Num(n)) => n,
        Some(_) => return Err(E_SCHEMA_TYPE),
        None => return Err(E_SCHEMA_FIELD),
    };
    let kind_v = match o.get(F_KIND) {
        Some(JsonValue::Num(n)) => n,
        Some(_) => return Err(E_SCHEMA_TYPE),
        None => return Err(E_SCHEMA_FIELD),
    };
    let payload_v = match o.get(F_PAYLOAD) {
        Some(JsonValue::Num(n)) => n,
        Some(_) => return Err(E_SCHEMA_TYPE),
        None => 0, // 可选字段缺省补零
    };
    if seq_v == 0 {
        return Err(E_SCHEMA_VALUE);
    }
    if kind_v == 0 || kind_v > 4 {
        return Err(E_SCHEMA_VALUE);
    }
    Ok((seq_v, ts_v, kind_v as u8, payload_v))
}

/// FNV-1a 单字折叠。
pub fn fnv1a(seed: u64, x: u64) -> u64 {
    let mut h = seed;
    h ^= x;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h
}

/// token 流指纹：FNV-1a 全折叠（导出批幂等判据的整流指纹）。
pub fn fnv_stream(tokens: &[u64]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < tokens.len() {
        h = fnv1a(h, tokens[i]);
        i += 1;
    }
    h
}

/// 序列化：[MAGIC, n, (tag, vtag, payload)×n, CHECK]。
/// vtag：0=Null 1=Bool 2=Num；CHECK = 前缀 FNV-1a 整流校验 token。
pub fn serialize(o: &JsonObj) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    out.push(TOK_MAGIC);
    out.push(o.fields.len() as u64);
    let mut i = 0;
    while i < o.fields.len() {
        let tag = o.fields[i].0;
        let v = o.fields[i].1;
        let (vtag, payload) = match v {
            JsonValue::Null => (0u64, 0u64),
            JsonValue::Bool(b) => (1u64, b as u64),
            JsonValue::Num(n) => (2u64, n),
        };
        out.push(tag as u64);
        out.push(vtag);
        out.push(payload);
        i += 1;
    }
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut j = 0;
    while j < out.len() {
        h = fnv1a(h, out[j]);
        j += 1;
    }
    out.push(h);
    out
}

/// 反解：先验整流校验 token，再逐字段还原。任何非法 token 拒收。
pub fn decode(tokens: &[u64]) -> Result<JsonObj, u16> {
    if tokens.len() < 3 {
        return Err(E_TOKEN_BAD);
    }
    if tokens[0] != TOK_MAGIC {
        return Err(E_TOKEN_BAD);
    }
    let n = tokens[1] as usize;
    if tokens.len() != 2 + n * 3 + 1 {
        return Err(E_TOKEN_BAD);
    }
    let body = tokens.len() - 1;
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < body {
        h = fnv1a(h, tokens[i]);
        i += 1;
    }
    if h != tokens[body] {
        return Err(E_TOKEN_BAD);
    }
    let mut o = JsonObj::new();
    let mut f = 0usize;
    while f < n {
        let tag = tokens[2 + f * 3] as u8;
        let vtag = tokens[3 + f * 3];
        let payload = tokens[4 + f * 3];
        let v = match vtag {
            0 => JsonValue::Null,
            1 => JsonValue::Bool(payload != 0),
            2 => JsonValue::Num(payload),
            _ => return Err(E_TOKEN_BAD),
        };
        o.set(tag, v);
        f += 1;
    }
    Ok(o)
}

/// 总日志中心导出账：append-only，seq 严格递增、ts 严格单调。
pub struct ExportHub {
    records: Vec<JsonObj>,
    seqs: Vec<u64>,
    tss: Vec<u64>,
    batch_fps: Vec<u64>,
}

impl ExportHub {
    pub fn new() -> Self {
        ExportHub {
            records: Vec::new(),
            seqs: Vec::new(),
            tss: Vec::new(),
            batch_fps: Vec::new(),
        }
    }

    /// 入闸：schema 全过 + 账序合法才收。
    pub fn submit(&mut self, o: &JsonObj) -> Result<(), u16> {
        let (seq, ts, _kind, _payload) = schema_check(o)?;
        if let Some(&last_seq) = self.seqs.last() {
            if seq <= last_seq {
                return Err(E_SEQ_ORDER);
            }
        }
        if let Some(&last_ts) = self.tss.last() {
            if ts <= last_ts {
                return Err(E_TS_ORDER);
            }
        }
        self.seqs.push(seq);
        self.tss.push(ts);
        self.records.push(o.clone());
        Ok(())
    }

    /// 导出批 token 流（逐记录拼接）。
    pub fn export_tokens(&self) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < self.records.len() {
            let t = serialize(&self.records[i]);
            let mut j = 0;
            while j < t.len() {
                out.push(t[j]);
                j += 1;
            }
            i += 1;
        }
        out
    }

    /// 导出：返回批指纹；同批重复导出幂等不重记。
    pub fn export(&mut self) -> Result<u64, u16> {
        let toks = self.export_tokens();
        let fp = fnv_stream(&toks);
        let mut dup = false;
        let mut i = 0;
        while i < self.batch_fps.len() {
            if self.batch_fps[i] == fp {
                dup = true;
                break;
            }
            i += 1;
        }
        if !dup {
            self.batch_fps.push(fp);
        }
        Ok(fp)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn batch_fp_count(&self) -> usize {
        self.batch_fps.len()
    }
}

/// 记录构造器：seq/ts/kind/payload 四字段齐备。
pub fn make_record(seq: u64, ts: u64, kind: u8, payload: u64) -> JsonObj {
    let mut o = JsonObj::new();
    o.set(F_SEQ, JsonValue::Num(seq));
    o.set(F_TS, JsonValue::Num(ts));
    o.set(F_KIND, JsonValue::Num(kind as u64));
    o.set(F_PAYLOAD, JsonValue::Num(payload));
    o
}

/// 导出夹具：10 条合法记录（seq 1..=10，ts 单调，kind 轮转，payload 平方）。
pub fn export_fixture() -> Vec<JsonObj> {
    let mut recs: Vec<JsonObj> = Vec::new();
    let mut i = 1u64;
    while i <= EXPORT_N as u64 {
        let kind = ((i - 1) % 4) as u8 + 1;
        recs.push(make_record(i, 1_000 + i * 5, kind, i * i));
        i += 1;
    }
    recs
}

/// 自例程判据：逐条落实 J1（导出 ×10 schema 校验全过），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let recs = export_fixture();

    // R1 导出 ×10：10 条记录逐条过 schema 闸入账，恰 10 条全过。
    let mut hub = ExportHub::new();
    let mut i = 0usize;
    while i < recs.len() {
        hub.submit(&recs[i])?;
        i += 1;
    }
    if hub.len() != EXPORT_N as usize {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 拒收矩阵：缺必填 / 类型不符 / 未知字段 / kind 越域 / seq 越域 /
    //    seq 重号 / ts 回退——七路病记录全部拒收且诊断码各归各位。
    let mut bad = make_record(11, 1_100, 1, 9);
    bad.set(F_KIND, JsonValue::Null); // 必填缺失（置 Null 等效缺失语义外的类型违规）
    match hub.submit(&bad) {
        Err(E_SCHEMA_TYPE) => {}
        _ => return Err(E_SCHEMA_TYPE),
    }
    let mut bad2 = JsonObj::new();
    bad2.set(F_SEQ, JsonValue::Bool(true));
    bad2.set(F_TS, JsonValue::Num(1_100));
    bad2.set(F_KIND, JsonValue::Num(1));
    match hub.submit(&bad2) {
        Err(E_SCHEMA_TYPE) => {}
        _ => return Err(E_SCHEMA_TYPE),
    }
    let mut bad3 = make_record(11, 1_100, 1, 9);
    bad3.set(9, JsonValue::Num(1)); // 未知字段
    match hub.submit(&bad3) {
        Err(E_SCHEMA_FIELD) => {}
        _ => return Err(E_SCHEMA_FIELD),
    }
    let mut stripped = JsonObj::new();
    stripped.set(F_SEQ, JsonValue::Num(11));
    stripped.set(F_TS, JsonValue::Num(1_100)); // 缺 F_KIND
    match hub.submit(&stripped) {
        Err(E_SCHEMA_FIELD) => {}
        _ => return Err(E_SCHEMA_FIELD),
    }
    match hub.submit(&make_record(11, 1_100, 9, 0)) {
        Err(E_SCHEMA_VALUE) => {} // kind 越域
        _ => return Err(E_SCHEMA_VALUE),
    }
    match hub.submit(&make_record(0, 1_100, 1, 0)) {
        Err(E_SCHEMA_VALUE) => {} // seq 越域
        _ => return Err(E_SCHEMA_VALUE),
    }
    match hub.submit(&make_record(10, 1_100, 1, 0)) {
        Err(E_SEQ_ORDER) => {} // seq 重号（10 已在账）
        _ => return Err(E_SEQ_ORDER),
    }
    match hub.submit(&make_record(11, 1_000, 1, 0)) {
        Err(E_TS_ORDER) => {} // ts 回退
        _ => return Err(E_TS_ORDER),
    }
    if hub.len() != EXPORT_N as usize {
        return Err(E_LEDGER_BAD); // 拒收不得入账
    }
    hits += 1;

    // R3 序列化确定性：10 条记录各序列化两次，逐 token 全等。
    i = 0;
    while i < recs.len() {
        let t1 = serialize(&recs[i]);
        let t2 = serialize(&recs[i]);
        if t1 != t2 || fnv_stream(&t1) != fnv_stream(&t2) {
            return Err(E_TOKEN_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R4 反解全等：导出流反解回的记录与原记录字段级全等（×10）。
    i = 0;
    while i < recs.len() {
        let toks = serialize(&recs[i]);
        let back = decode(&toks)?;
        if back != recs[i] {
            return Err(E_ROUNDTRIP);
        }
        i += 1;
    }
    hits += 1;

    // R5 整流校验：payload token 被篡改一位 → 反解必拒收。
    let mut tam = serialize(&recs[3]);
    let last = tam.len() - 1;
    let chk = tam[last];
    tam[5] = tam[5].wrapping_add(1); // 篡改正文字段
    match decode(&tam) {
        Err(E_TOKEN_BAD) => {}
        _ => return Err(E_TOKEN_BAD),
    }
    tam[5] = tam[5].wrapping_sub(1);
    tam[last] = chk.wrapping_add(1); // 只篡改校验 token
    match decode(&tam) {
        Err(E_TOKEN_BAD) => {}
        _ => return Err(E_TOKEN_BAD),
    }
    hits += 1;

    // R6 导出幂等：同批二次导出同指纹且不重记；token 流长度恰 15×10。
    let fp1 = hub.export()?;
    if hub.batch_fp_count() != 1 {
        return Err(E_LEDGER_BAD);
    }
    let fp2 = hub.export()?;
    if fp1 != fp2 || hub.batch_fp_count() != 1 {
        return Err(E_LEDGER_BAD);
    }
    if hub.export_tokens().len() != 15 * EXPORT_N as usize {
        return Err(E_TOKEN_BAD);
    }
    hits += 1;

    // R7 账序复核：账内 seq 严格递增、ts 严格单调（append-only 账面）。
    let mut s = 1;
    while s < hub.len() {
        if hub.seqs[s] <= hub.seqs[s - 1] || hub.tss[s] <= hub.tss[s - 1] {
            return Err(E_SEQ_ORDER);
        }
        s += 1;
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 9] = [
        E_SCHEMA_TYPE,
        E_SCHEMA_FIELD,
        E_SCHEMA_VALUE,
        E_SEQ_ORDER,
        E_TS_ORDER,
        E_TOKEN_BAD,
        E_ROUNDTRIP,
        E_LEDGER_BAD,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
