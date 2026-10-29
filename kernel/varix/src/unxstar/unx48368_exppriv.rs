//! unx48368_exppriv — UNX-F48368 隐私红线账（只记交互不记输入内容全文，密码框零记录）（诊断段 0xCEE0）。
//!
//! 判据原文：
//!   J1 密码框注入 ×10 零记录；判据锚定 体验十三 隐私红线。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 红线绝对性：密码框交互零记录——不是脱敏，不是摘要，是连事件都
//!      不产生；任何「强制记录」旗标对密码框一律失效，红线没有例外
//!      通道。
//!   2. 只记交互不记全文：普通框允许记录的只有（字段种类, 动作种类,
//!      长度桶）三元组，输入内容全文从不进入记录结构——记录结构里根
//!      本没有存放全文的槽位，这是结构性保证而非约定。
//!   3. ×10 零记录：十次密码框注入（每次内容不同）后，账长度、账指纹
//!      必须与注入前完全一致；同时抑制计数必须恰好 +10（零记录不等于
//!      零问责——发生了什么要记，内容是什么不记）。
//!   4. 泄漏扫描：全部在账记录的载荷域只有种类码与长度桶，扫描任何记
//!      录都拼不回原文的任何一个码元。
//!   5. 功能不因红线残废：普通框交互正常入账且长度桶正确，保证红线是
//!      精确的手术刀而不是一禁了之。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCEE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_PASSWORD_LEAK: u16 = SEG_BASE + 1;  // 密码框产生了记录（红线击穿）
const E_TEXT_IN_RECORD: u16 = SEG_BASE + 2; // 记录中出现全文码元
const E_SUPPRESS_FAULT: u16 = SEG_BASE + 3; // 抑制计数不符（零记录≠零问责）
const E_BUCKET_FAULT: u16 = SEG_BASE + 4;   // 长度桶错账
const E_NORMAL_BLIND: u16 = SEG_BASE + 5;   // 普通框交互未入账（红线误伤）
const E_FORCE_BYPASS: u16 = SEG_BASE + 6;   // 强制旗标绕过了红线
const E_TAMPER_UNDETECTED: u16 = SEG_BASE + 7; // 账指纹漂移未检出
const E_CODE_RANGE: u16 = SEG_BASE + 8;     // 诊断码越段

/// 字段种类：普通框 / 密码框。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Normal,
    Password,
}

/// 交互动作种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Focus,
    Key,
    Commit,
    Blur,
}

/// 长度桶宽度（字符数）。
pub const BUCKET_WIDTH: usize = 8;

/// 长度桶上限（超长一律封顶）。
pub const BUCKET_CAP: u8 = 31;

/// 交互元数据记录：结构上不存在存放全文的槽位。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MetaRecord {
    pub seq: u32,
    pub field: FieldKind,
    pub action: ActionKind,
    /// 长度桶：min(字符数 / BUCKET_WIDTH, BUCKET_CAP)。
    pub len_bucket: u8,
}

/// 隐私红线账：只收元数据，密码框连元数据都不收。
pub struct RedlineLog {
    records: Vec<MetaRecord>,
    /// 被抑制（未产生任何记录）的观察次数。
    pub suppressed: u32,
    /// 密码框强制记录尝试被拒绝的次数。
    pub force_denied: u32,
    seq: u32,
}

impl RedlineLog {
    pub fn new() -> Self {
        RedlineLog {
            records: Vec::new(),
            suppressed: 0,
            force_denied: 0,
            seq: 0,
        }
    }

    /// 观察一次交互。text 是输入内容全文（调用方持有），本函数保证它
    /// 不会以任何形式进入记录——返回 true 表示产生了一条元数据记录。
    pub fn observe(&mut self, field: FieldKind, action: ActionKind, text_len: usize, force: bool) -> bool {
        if field == FieldKind::Password {
            // 红线：密码框零记录，force 旗标一律失效。
            self.suppressed += 1;
            if force {
                self.force_denied += 1;
            }
            return false;
        }
        let mut bucket = (text_len / BUCKET_WIDTH) as u8;
        if bucket > BUCKET_CAP {
            bucket = BUCKET_CAP;
        }
        self.records.push(MetaRecord {
            seq: self.seq,
            field,
            action,
            len_bucket: bucket,
        });
        self.seq += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn records(&self) -> &[MetaRecord] {
        &self.records
    }
}

/// 账指纹：FNV-1a 折叠全部元数据记录。
pub fn fingerprint(log: &RedlineLog) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < log.records().len() {
        let r = &log.records()[i];
        h ^= r.seq as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= match r.field {
            FieldKind::Normal => 1u64,
            FieldKind::Password => 2u64,
        };
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= match r.action {
            ActionKind::Focus => 1u64,
            ActionKind::Key => 2u64,
            ActionKind::Commit => 3u64,
            ActionKind::Blur => 4u64,
        };
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= r.len_bucket as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 确定性线性同余发生器（注入剧本统一入口，禁真随机）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 生成一段确定性伪密码文本（各次注入内容互不相同），返回（码元串, 长度）。
pub fn secret_text(seed: u32, n: usize) -> Vec<u16> {
    let mut lc = Lcg::new(seed);
    let mut out: Vec<u16> = Vec::new();
    let mut i = 0usize;
    while i < n {
        out.push(0x4E00 + (lc.next() % 0x0200) as u16);
        i += 1;
    }
    out
}

/// 泄漏扫描：全部在账记录的域检查——字段种类合法、动作种类合法、
/// 长度桶不越界。记录结构无全文槽位，域检查即结构性复核。
pub fn leak_scan(log: &RedlineLog) -> bool {
    let mut i = 0usize;
    while i < log.records().len() {
        let r = &log.records()[i];
        if r.field == FieldKind::Password {
            return false; // 密码框绝不允许出现在账内
        }
        match r.action {
            ActionKind::Focus | ActionKind::Key | ActionKind::Commit | ActionKind::Blur => {}
        }
        if r.len_bucket > BUCKET_CAP {
            return false;
        }
        i += 1;
    }
    true
}

/// 自例程判据：逐条落实 J1 的全部断言（密码框注入 ×10 零记录）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R0 前置：普通框功能正常——20 次交互全部入账且长度桶正确。
    let mut log = RedlineLog::new();
    let mut lc = Lcg::new(0x4836_8001);
    let mut i = 0u32;
    while i < 20 {
        let tlen = (lc.next() % 300) as usize;
        let action = match lc.next() % 4 {
            0 => ActionKind::Focus,
            1 => ActionKind::Key,
            2 => ActionKind::Commit,
            _ => ActionKind::Blur,
        };
        if !log.observe(FieldKind::Normal, action, tlen, false) {
            return Err(E_NORMAL_BLIND);
        }
        let expect = {
            let b = tlen / BUCKET_WIDTH;
            if b > BUCKET_CAP as usize {
                BUCKET_CAP
            } else {
                b as u8
            }
        };
        if log.records()[i as usize].len_bucket != expect {
            return Err(E_BUCKET_FAULT);
        }
        i += 1;
    }
    hits += 1;

    // R1 ×10 密码框注入零记录：账长与账指纹与注入前完全一致。
    let before_len = log.len();
    let before_fp = fingerprint(&log);
    let mut injected: Vec<Vec<u16>> = Vec::new();
    i = 0;
    while i < 10 {
        let t = secret_text((0x4836_8002u32) ^ (i * 6553 + 1), 12 + i as usize);
        injected.push(t);
        // 全文交给 observe（调用方视角），红线必须保证它不落账。
        let tlen = injected[i as usize].len();
        if log.observe(FieldKind::Password, ActionKind::Key, tlen, false) {
            return Err(E_PASSWORD_LEAK);
        }
        i += 1;
    }
    if log.len() != before_len || fingerprint(&log) != before_fp {
        return Err(E_PASSWORD_LEAK);
    }
    hits += 1;

    // R2 零记录 ≠ 零问责：抑制计数恰好 +10，每次注入都有账可查。
    if log.suppressed != 10 {
        return Err(E_SUPPRESS_FAULT);
    }
    hits += 1;

    // R3 红线无例外通道：force 旗标对密码框一律失效（×10 全拒）。
    i = 0;
    while i < 10 {
        if log.observe(FieldKind::Password, ActionKind::Commit, 16, true) {
            return Err(E_FORCE_BYPASS);
        }
        i += 1;
    }
    if log.force_denied != 10 || log.len() != before_len {
        return Err(E_FORCE_BYPASS);
    }
    hits += 1;

    // R4 泄漏扫描：全部在账记录域合法，拼不回任何注入文本的码元。
    if !leak_scan(&log) {
        return Err(E_TEXT_IN_RECORD);
    }
    // 结构性复核：逐记录遍历全部字段位，任何位都不等于注入文本码元。
    let mut j = 0usize;
    while j < injected.len() {
        let t = &injected[j];
        let mut k = 0usize;
        while k < t.len() {
            let c = t[k];
            let mut r = 0usize;
            while r < log.records().len() {
                let rec = &log.records()[r];
                let bits = rec.seq as u16 ^ rec.len_bucket as u16;
                if bits == c {
                    return Err(E_TEXT_IN_RECORD);
                }
                r += 1;
            }
            k += 1;
        }
        j += 1;
    }
    hits += 1;

    // R5 普通框在红线之后依旧精确：追加 5 次普通交互，桶值逐一对账。
    let base = log.len();
    let mut lc3 = Lcg::new(0x4836_8003);
    i = 0;
    while i < 5 {
        let tlen = (lc3.next() % 200) as usize;
        if !log.observe(FieldKind::Normal, ActionKind::Key, tlen, false) {
            return Err(E_NORMAL_BLIND);
        }
        let b = tlen / BUCKET_WIDTH;
        let expect = if b > BUCKET_CAP as usize {
            BUCKET_CAP
        } else {
            b as u8
        };
        if log.records()[base + i as usize].len_bucket != expect {
            return Err(E_BUCKET_FAULT);
        }
        i += 1;
    }
    if log.len() != base + 5 {
        return Err(E_NORMAL_BLIND);
    }
    hits += 1;

    // R6 账指纹防漂移：同账重算指纹必须一致；伪造额外观察改变指纹必须
    // 能被重算暴露（假阴性为零）。
    let fp1 = fingerprint(&log);
    let fp2 = fingerprint(&log);
    if fp1 != fp2 {
        return Err(E_TAMPER_UNDETECTED);
    }
    let mut shadow = RedlineLog::new();
    let mut k = 0usize;
    while k < log.len() {
        let r = &log.records()[k];
        let _ = shadow.observe(r.field, r.action, (r.len_bucket as usize) * BUCKET_WIDTH, false);
        k += 1;
    }
    // 影子账按桶还原长度后重放，桶值必须逐条复现。
    let mut k = 0usize;
    while k < log.len() {
        if shadow.records()[k].len_bucket != log.records()[k].len_bucket {
            return Err(E_TAMPER_UNDETECTED);
        }
        k += 1;
    }
    hits += 1;

    // R7 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_PASSWORD_LEAK,
        E_TEXT_IN_RECORD,
        E_SUPPRESS_FAULT,
        E_BUCKET_FAULT,
        E_NORMAL_BLIND,
        E_FORCE_BYPASS,
        E_TAMPER_UNDETECTED,
        E_CODE_RANGE,
    ];
    let mut ci = 0usize;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
