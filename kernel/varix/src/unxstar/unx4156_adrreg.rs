//! unx4156_adrreg — UNX-F4156 接口冻结 ADR 登记（交付 AI-93）（诊断段 0x9540）。
//!
//! 判据原文：
//!   J1 ADR 草案三件（背景/决策/影响面）齐备落盘。
//!   J2 登记项与冻结快照哈希一致可核对。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. ADR 三件闭集：背景 CONTEXT / 决策 DECISION / 影响面 IMPACT，
//!      件号 0..2 定编，缺件不得冻结。
//!   2. 内容指纹：正文按 FNV-1a 32 位整数哈希落账，落盘即指纹入账。
//!   3. 冻结快照哈希：三件指纹按件号序折叠成快照哈希，冻结后任何
//!      再登记拒收（冻结不可变红线）。
//!   4. 核对口径：外部持有快照哈希与登记账重算哈希逐位一致（判据 J2）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x9540;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_KIND: u16 = SEG_BASE + 1;   // 件号越界（三件闭集外）
const E_EMPTY_BODY: u16 = SEG_BASE + 2; // 空正文拒收
const E_DUP_DOC: u16 = SEG_BASE + 3;    // 同件号重复登记
const E_FROZEN_LOCK: u16 = SEG_BASE + 4;// 冻结后再登记（不可变红线）
const E_MISSING_DOC: u16 = SEG_BASE + 5;// 缺件冻结被拒
const E_HASH_MISMATCH: u16 = SEG_BASE + 6; // 快照哈希核对不一致
const E_BAD_STATE: u16 = SEG_BASE + 7;  // 登记状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 8;  // 写回摘要为空

/// ADR 三件件号闭集。
pub const ADR_CONTEXT: usize = 0; // 背景
pub const ADR_DECISION: usize = 1; // 决策
pub const ADR_IMPACT: usize = 2;  // 影响面
pub const ADR_COUNT: usize = 3;

/// 正文容量上限（整数落盘口径）。
pub const BODY_CAP: usize = 64;

/// 登记状态机：Draft 草案期 / Frozen 已冻结。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdrState {
    Draft,
    Frozen,
}

/// 单件 ADR：正文定长槽 + 指纹。
pub struct AdrDoc {
    pub kind: usize,           // 件号 0..2
    pub body: [u8; BODY_CAP],  // 正文槽（0 填充）
    pub body_len: usize,       // 正文长度
    pub content_hash: u32,     // FNV-1a 内容指纹
}

/// ADR 登记簿：三件账 + 冻结快照哈希。
pub struct AdrRegistry {
    docs: [Option<AdrDoc>; ADR_COUNT],
    pub state: AdrState,
    pub snapshot_hash: u32,     // 冻结快照哈希（0=未冻结）
    pub register_total: u32,    // 登记成功计数
    pub freeze_epoch: u32,      // 冻结代次
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5; // FNV-1a 偏移基
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x01000193); // FNV-1a 质数
        i += 1;
    }
    h
}

impl AdrRegistry {
    pub fn new() -> AdrRegistry {
        AdrRegistry {
            docs: [None, None, None],
            state: AdrState::Draft,
            snapshot_hash: 0,
            register_total: 0,
            freeze_epoch: 0,
        }
    }

    /// 登记单件：正文非空、件号在闭集内、未冻结、不重复。
    pub fn register_doc(&mut self, kind: usize, body: &[u8]) -> Result<(), u16> {
        if kind >= ADR_COUNT {
            return Err(E_BAD_KIND);
        }
        if body.is_empty() || body.len() > BODY_CAP {
            return Err(E_EMPTY_BODY);
        }
        if self.state == AdrState::Frozen {
            return Err(E_FROZEN_LOCK);
        }
        if self.docs[kind].is_some() {
            return Err(E_DUP_DOC);
        }
        let mut slot = [0u8; BODY_CAP];
        let mut i = 0usize;
        while i < body.len() {
            slot[i] = body[i];
            i += 1;
        }
        let h = fnv1a(body);
        self.docs[kind] = Some(AdrDoc { kind, body: slot, body_len: body.len(), content_hash: h });
        self.register_total += 1;
        Ok(())
    }

    /// 三件齐备核验（判据 J1 齐备面）。
    pub fn docs_complete(&self) -> Result<(), u16> {
        let mut k = 0usize;
        while k < ADR_COUNT {
            if self.docs[k].is_none() {
                return Err(E_MISSING_DOC);
            }
            k += 1;
        }
        Ok(())
    }

    /// 快照哈希重算：三件指纹按件号序折叠。
    pub fn recompute_snapshot(&self) -> Result<u32, u16> {
        self.docs_complete()?;
        let mut h: u32 = 0x811C9DC5;
        let mut k = 0usize;
        while k < ADR_COUNT {
            match &self.docs[k] {
                Some(d) => {
                    h = h.wrapping_mul(0x01000193) ^ d.content_hash;
                }
                None => return Err(E_MISSING_DOC),
            }
            k += 1;
        }
        Ok(h)
    }

    /// 冻结：三件齐备才可冻结，快照哈希落账（判据 J1/J2）。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.state == AdrState::Frozen {
            return Err(E_FROZEN_LOCK);
        }
        let h = self.recompute_snapshot()?;
        self.snapshot_hash = h;
        self.state = AdrState::Frozen;
        self.freeze_epoch += 1;
        Ok(h)
    }

    /// 核对：外部持有哈希与登记账重算值逐位一致（判据 J2）。
    pub fn verify_snapshot(&self, expect: u32) -> Result<(), u16> {
        if self.snapshot_hash == 0 {
            return Err(E_BAD_STATE);
        }
        let cur = self.recompute_snapshot()?;
        if cur != expect || cur != self.snapshot_hash {
            return Err(E_HASH_MISMATCH);
        }
        Ok(())
    }

    /// 登记项可读：按件号取指纹（核对用）。
    pub fn doc_hash(&self, kind: usize) -> Result<u32, u16> {
        if kind >= ADR_COUNT {
            return Err(E_BAD_KIND);
        }
        match &self.docs[kind] {
            Some(d) => Ok(d.content_hash),
            None => Err(E_MISSING_DOC),
        }
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 三件登记冻结 → R2 边界 → R3 注入拦截 → R4 哈希核对。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：三件登记 → 冻结 → 核对一致。
    let mut r = AdrRegistry::new();
    r.register_doc(ADR_CONTEXT, b"bg: vfs freeze")?;
    r.register_doc(ADR_DECISION, b"dc: lock tables")?;
    r.register_doc(ADR_IMPACT, b"im: b2-b5")?;
    r.docs_complete()?;
    let h = r.freeze()?;
    r.verify_snapshot(h)?;
    r.writeback(true, 0x30)?;
    hits += 1;

    // R2 边界：件号越界 / 空正文 / 缺件冻结。
    let mut r = AdrRegistry::new();
    if r.register_doc(3, b"x").is_ok() {
        return Err(E_BAD_KIND);
    }
    if r.register_doc(ADR_CONTEXT, b"").is_ok() {
        return Err(E_EMPTY_BODY);
    }
    if r.freeze().is_ok() {
        return Err(E_MISSING_DOC);
    }
    hits += 1;

    // R3 注入拦截：重复登记 / 冻结后再登记 / 空摘要写回。
    let mut r = AdrRegistry::new();
    r.register_doc(ADR_CONTEXT, b"a")?;
    if r.register_doc(ADR_CONTEXT, b"a2").is_ok() {
        return Err(E_DUP_DOC);
    }
    r.register_doc(ADR_DECISION, b"b")?;
    r.register_doc(ADR_IMPACT, b"c")?;
    r.freeze()?;
    if r.register_doc(ADR_IMPACT, b"zz").is_ok() {
        return Err(E_FROZEN_LOCK);
    }
    if r.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    hits += 1;

    // R4 哈希核对：内容变一位则快照必变；原快照仍可对上原账。
    let mut r = AdrRegistry::new();
    r.register_doc(ADR_CONTEXT, b"body-one")?;
    r.register_doc(ADR_DECISION, b"body-two")?;
    r.register_doc(ADR_IMPACT, b"body-three")?;
    let h1 = r.recompute_snapshot()?;
    r.freeze()?;
    r.verify_snapshot(h1)?;
    // 篡改感知：外部期望值换成错哈希必须核对失败。
    if r.verify_snapshot(h1 ^ 1).is_ok() {
        return Err(E_HASH_MISMATCH);
    }
    if r.doc_hash(ADR_DECISION)? == 0 {
        return Err(E_BAD_STATE);
    }
    hits += 1;

    // R5 追加回归：正文容量边界 + 三件指纹逐一核对 + 冻结代次。
    let mut r = AdrRegistry::new();
    let full = [b'K'; BODY_CAP]; // 恰容量正文
    r.register_doc(ADR_CONTEXT, &full)?;
    let over = [b'L'; BODY_CAP + 1];
    if r.register_doc(ADR_DECISION, &over).is_ok() {
        return Err(E_EMPTY_BODY);
    }
    r.register_doc(ADR_DECISION, b"ok-decision")?;
    r.register_doc(ADR_IMPACT, b"ok-impact")?;
    // 三件指纹逐件可查且非零。
    if r.doc_hash(ADR_CONTEXT)? == 0 || r.doc_hash(ADR_DECISION)? == 0 {
        return Err(E_MISSING_DOC);
    }
    let h = r.recompute_snapshot()?;
    r.freeze()?;
    if r.freeze_epoch != 1 {
        return Err(E_BAD_STATE);
    }
    r.verify_snapshot(h)?;
    // 重算与落账一致（判据 J2 复核面）。
    if r.recompute_snapshot()? != r.snapshot_hash {
        return Err(E_HASH_MISMATCH);
    }
    if r.writeback(true, 0x11).is_err() {
        return Err(E_EMPTY_SUM);
    }
    hits += 1;

    // R6 回归：冻结前指纹随正文变化，冻结后核对锁定。
    let mut r2 = AdrRegistry::new();
    r2.register_doc(ADR_CONTEXT, b"v1")?;
    r2.register_doc(ADR_DECISION, b"v1")?;
    r2.register_doc(ADR_IMPACT, b"v1")?;
    let h1 = r2.recompute_snapshot()?;
    // 另册同件异文：快照必随内容变化。
    let mut r3 = AdrRegistry::new();
    r3.register_doc(ADR_CONTEXT, b"v2")?;
    r3.register_doc(ADR_DECISION, b"v1")?;
    r3.register_doc(ADR_IMPACT, b"v1")?;
    let h2 = r3.recompute_snapshot()?;
    if h1 == h2 {
        return Err(E_HASH_MISMATCH); // 内容变则快照必变
    }
    if r3.register_doc(ADR_IMPACT, b"zz").is_ok() {
        return Err(E_DUP_DOC);
    }
    r2.freeze()?;
    r2.verify_snapshot(h1)?;
    if r2.register_total != 3 {
        return Err(E_MISSING_DOC);
    }
    hits += 1;

    Ok(hits)
}
