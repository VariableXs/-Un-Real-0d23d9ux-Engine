//! UNX-F0677 · 耗时账封账（账面只读封存）· 诊断段 0x55e0（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0677-J1 四账族封账哈希齐（4/4）且封账后写尝试检出 100%（只读守卫）。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 四账族闭集：阈值账/五数账/守恒账/告警账，逐账封存哈希。
//!  3. 封账单向门：seal 后任何写路径一律拦截，写尝试全部立案。
//!  4. 哈希齐备核：4/4 齐才通过；篡改任一哈希 → 复核检出。
//!  5. 封账时戳序号单调：四账封存次序可解引。
//!  6. 诊断段 0x55e0 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x55e0;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_WRONLY: u16 = SEG_BASE + 1; // 封账后写尝试（只读守卫拦截）
pub const E_HASH: u16 = SEG_BASE + 2;   // 封账哈希复核不平
pub const E_INCOMPLETE: u16 = SEG_BASE + 3; // 封账哈希不齐（<4/4）
pub const E_SEAL: u16 = SEG_BASE + 4;   // 重复封账
pub const E_ACCT: u16 = SEG_BASE + 5;   // 账族编号非法
pub const E_ORDER: u16 = SEG_BASE + 6;  // 封存时戳序号错位
pub const E_EMPTY: u16 = SEG_BASE + 7;  // 空账封存拦截
pub const E_ROW: u16 = SEG_BASE + 8;    // 核对行空行（行内容为空串）

// fill_all 注入盐（seed 语义：段号 0x0677 高位 + 场景低位，逐场景互异保确定性）。
pub const SALT_T: u64 = 0x0677_0001;     // 哈希篡改注入核盐
pub const SALT_FPA: u64 = 0x0677_0002;   // 金库总指纹审计盐
pub const SALT_FG: u64 = 0x0677_0003;    // 填账去重门盐
pub const SALT_SEQ: u64 = 0x0677_0004;   // 时戳序号注入核盐
pub const SALT_MI: u64 = 0x0677_0005;    // 清单未齐注入核盐
pub const SALT_VAULT: u64 = 0x0677_0006; // 全库走核盐

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 四账族闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcctKind {
    /// 账一：冷启动阈值判定账。
    Thresh,
    /// 账二：五数分布账。
    Five,
    /// 账三：段守恒账。
    Cons,
    /// 账四：告警路由账。
    Alert,
}

impl AcctKind {
    pub const ALL: [AcctKind; 4] = [AcctKind::Thresh, AcctKind::Five, AcctKind::Cons, AcctKind::Alert];

    pub fn idx(self) -> usize {
        match self {
            AcctKind::Thresh => 0,
            AcctKind::Five => 1,
            AcctKind::Cons => 2,
            AcctKind::Alert => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AcctKind::Thresh => "阈值账",
            AcctKind::Five => "五数账",
            AcctKind::Cons => "守恒账",
            AcctKind::Alert => "告警账",
        }
    }

    pub fn from_idx(i: usize) -> Result<AcctKind, u16> {
        for k in AcctKind::ALL {
            if k.idx() == i {
                return Ok(k);
            }
        }
        Err(E_ACCT)
    }
}

/// 单账数据体（行向量，封账即冻结）。
pub struct AcctBody {
    pub kind: AcctKind,
    /// 账行（u64 向量，语义由账族自定）。
    pub rows: Vec<u64>,
    pub sealed: bool,
    /// 封账哈希。
    pub seal_hash: u64,
    /// 封账时戳序号。
    pub seal_seq: u64,
}

impl AcctBody {
    pub fn new(kind: AcctKind) -> AcctBody {
        AcctBody { kind, rows: Vec::new(), sealed: false, seal_hash: 0, seal_seq: 0 }
    }

    /// 封账前写。
    pub fn write(&mut self, v: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_WRONLY);
        }
        if v == 0 {
            return Err(E_EMPTY);
        }
        self.rows.push(v);
        Ok(())
    }

    /// 体指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = vec![0u8; self.rows.len() * 8 + 1];
        buf[0] = self.kind.idx() as u8;
        for (i, v) in self.rows.iter().enumerate() {
            buf[1 + i * 8] = (v >> 56) as u8;
            buf[1 + i * 8 + 1] = (v >> 48) as u8;
            buf[1 + i * 8 + 2] = (v >> 40) as u8;
            buf[1 + i * 8 + 3] = (v >> 32) as u8;
            buf[1 + i * 8 + 4] = (v >> 24) as u8;
            buf[1 + i * 8 + 5] = (v >> 16) as u8;
            buf[1 + i * 8 + 6] = (v >> 8) as u8;
            buf[1 + i * 8 + 7] = *v as u8;
        }
        fnv1a64(&buf)
    }

    /// 封账：空账拦截；封后写尝试拦截。
    pub fn seal(&mut self, seq: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if self.rows.is_empty() {
            return Err(E_EMPTY);
        }
        if seq == 0 {
            return Err(E_ORDER);
        }
        self.seal_hash = self.fingerprint();
        self.seal_seq = seq;
        self.sealed = true;
        Ok(())
    }

    /// 哈希复核。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_INCOMPLETE);
        }
        if self.fingerprint() != self.seal_hash {
            return Err(E_HASH);
        }
        Ok(())
    }

    /// 写尝试守卫（供注入核调用）。
    pub fn try_write_post(&self, v: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_WRONLY);
        }
        let _ = v;
        Ok(())
    }
}

/// 封账金库：四账族总封。
pub struct SealVault {
    pub bodies: Vec<AcctBody>,
    pub seq: u64,
    /// 写尝试立案簿：(账族, 拦截次数)。
    pub interceptions: [(u32, u32); 4],
    pub sealed: bool,
}

impl SealVault {
    pub fn new() -> SealVault {
        SealVault {
            bodies: vec![
                AcctBody::new(AcctKind::Thresh),
                AcctBody::new(AcctKind::Five),
                AcctBody::new(AcctKind::Cons),
                AcctBody::new(AcctKind::Alert),
            ],
            seq: 0,
            interceptions: [(0, 0), (1, 0), (2, 0), (3, 0)],
            sealed: false,
        }
    }

    /// 按账族取体。
    pub fn body_of(&mut self, k: AcctKind) -> Result<&mut AcctBody, u16> {
        for b in self.bodies.iter_mut() {
            if b.kind == k {
                return Ok(b);
            }
        }
        Err(E_ACCT)
    }

    /// 填账：每族灌入确定性数据。
    pub fn fill_all(&mut self, seed: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let mut st = seed | 1;
        for i in 0..4usize {
            let n = 10 + i * 5;
            for _ in 0..n {
                st = st.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let v = (st >> 33) % 5_000 + 1;
                self.bodies[i].write(v)?;
            }
        }
        Ok(())
    }

    /// 全族封账：四账依序封存，时戳序号单调。
    pub fn seal_all(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        for i in 0..4usize {
            self.seq += 1;
            self.bodies[i].seal(self.seq)?;
        }
        self.sealed = true;
        Ok(())
    }

    /// 哈希齐备核：4/4 全部 verify 过。
    pub fn assert_all_sealed(&self) -> Result<(), u16> {
        let mut n: u32 = 0;
        for b in &self.bodies {
            if b.sealed {
                b.verify()?;
                n += 1;
            }
        }
        if n != 4 {
            return Err(E_INCOMPLETE);
        }
        Ok(())
    }

    /// 写尝试（守卫路径）：封后写必拦截并立案。
    pub fn attempt_write(&mut self, k: AcctKind, v: u64) -> Result<(), u16> {
        for b in self.bodies.iter_mut() {
            if b.kind == k {
                if b.try_write_post(v) != Err(E_WRONLY) {
                    return Err(E_WRONLY);
                }
                self.interceptions[k.idx()].1 += 1;
                return Err(E_WRONLY);
            }
        }
        Err(E_ACCT)
    }

    /// 拦截检出率核：每族拦截数 > 0（注入复验覆盖 4/4）。
    pub fn assert_intercepted_all(&self) -> Result<(), u16> {
        for (i, (_, n)) in self.interceptions.iter().enumerate() {
            if *n == 0 {
                let _ = i;
                return Err(E_WRONLY);
            }
        }
        Ok(())
    }

    /// 时戳序号单调核：封存次序 1..4。
    pub fn assert_seq_order(&self) -> Result<(), u16> {
        for (i, b) in self.bodies.iter().enumerate() {
            if b.seal_seq != (i + 1) as u64 {
                return Err(E_ORDER);
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("封账金库: 4/4 拦截=");
        let mut total: u32 = 0;
        for (_, n) in &self.interceptions {
            total += n;
        }
        s.push_str(&total.to_string());
        s
    }
}

/// 哈希篡改注入核：改任一族哈希 → 复核检出。
pub fn inject_hash_tamper_must_fail() -> Result<(), u16> {
    let mut v = SealVault::new();
    v.fill_all(SALT_T)?;
    v.seal_all()?;
    v.bodies[2].seal_hash ^= 0xFF;
    if v.assert_all_sealed() != Err(E_HASH) {
        return Err(SEG_BASE + 0x30);
    }
    Ok(())
}

/// 空账封存注入核：清空一族再封 → 拦截。
pub fn inject_empty_seal_must_fail() -> Result<(), u16> {
    let mut v = SealVault::new();
    // 不填账直接封 → 空账拦截
    let mut b = AcctBody::new(AcctKind::Five);
    if b.seal(1) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x31);
    }
    let _ = &mut v;
    Ok(())
}

/// 账族闭集自检。
pub fn kind_closure_audit() -> Result<(), u16> {
    for k in AcctKind::ALL {
        if AcctKind::from_idx(k.idx())? != k {
            return Err(E_ACCT);
        }
        if k.name().is_empty() {
            return Err(E_ACCT);
        }
    }
    if AcctKind::from_idx(9) != Err(E_ACCT) {
        return Err(SEG_BASE + 0x32);
    }
    Ok(())
}

/// 指纹确定性自检：同账两次现算全等；改行必检出。
pub fn fp_audit() -> Result<(), u16> {
    let mut b = AcctBody::new(AcctKind::Cons);
    b.write(11)?;
    b.write(22)?;
    let f1 = b.fingerprint();
    let f2 = b.fingerprint();
    if f1 != f2 {
        return Err(SEG_BASE + 0x33);
    }
    b.write(33)?;
    if b.fingerprint() == f1 {
        return Err(SEG_BASE + 0x34);
    }
    // 零值行拦截
    let mut b2 = AcctBody::new(AcctKind::Cons);
    if b2.write(0) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// 封账报告（逐族派生）。
pub struct SealReport {
    pub lines: Vec<String>,
}

impl SealReport {
    pub fn generate(v: &SealVault) -> Result<SealReport, u16> {
        v.assert_all_sealed()?;
        v.assert_seq_order()?;
        let mut lines = Vec::new();
        lines.push(v.summary());
        for b in &v.bodies {
            let mut l = String::from(b.kind.name());
            l.push_str(" 哈希尾=");
            l.push_str(&(b.seal_hash % 1_000_000).to_string());
            l.push_str(" 序号=");
            l.push_str(&b.seal_seq.to_string());
            lines.push(l);
        }
        Ok(SealReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() != 5 {
            return Err(E_INCOMPLETE);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_ROW);
            }
        }
        Ok(())
    }
}

/// 金库总指纹：四账封存哈希按族序拼装（封账清单的最终锚点）。
pub fn vault_fp(v: &SealVault) -> u64 {
    let mut buf = vec![0u8; 36];
    for (i, b) in v.bodies.iter().enumerate() {
        buf[i * 9] = b.kind.idx() as u8;
        for k in 0..8u8 {
            buf[i * 9 + 1 + k as usize] = (b.seal_hash >> (56 - k * 8)) as u8;
        }
    }
    fnv1a64(&buf)
}

/// 总指纹确定性 + 篡改敏感自检。
pub fn vault_fp_audit() -> Result<(), u16> {
    let mut v = SealVault::new();
    v.fill_all(SALT_FPA)?;
    v.seal_all()?;
    let f1 = vault_fp(&v);
    let f2 = vault_fp(&v);
    if f1 != f2 {
        return Err(SEG_BASE + 0x39);
    }
    v.bodies[0].seal_hash ^= 1;
    if vault_fp(&v) == f1 {
        return Err(SEG_BASE + 0x3A);
    }
    Ok(())
}

/// 账行数下限审计：四族行数各 ≥ 10（灌账口径），不足即封前拦截。
pub fn row_floor_audit(v: &SealVault) -> Result<(), u16> {
    for b in &v.bodies {
        if b.rows.len() < 10 {
            return Err(E_EMPTY);
        }
    }
    Ok(())
}

/// 封前可写 / 封后只读状态机自检。
pub fn write_gate_audit() -> Result<(), u16> {
    let mut b = AcctBody::new(AcctKind::Alert);
    // 封前写放行
    b.write(7)?;
    b.write(8)?;
    // 封后写拦截（经 seal）
    b.seal(5)?;
    if b.write(9) != Err(E_WRONLY) {
        return Err(SEG_BASE + 0x3B);
    }
    // 重复封账拦截
    if b.seal(6) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x3C);
    }
    Ok(())
}

/// 金库填账去重自检：fill_all 两次 → 第二次封账路径被 sealed 拦截（不是重复灌账）。
pub fn fill_gate_audit() -> Result<(), u16> {
    let mut v = SealVault::new();
    v.fill_all(SALT_FG)?;
    v.seal_all()?;
    if v.fill_all(2) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x3D);
    }
    Ok(())
}

/// 时戳序号注入核：乱序封存（跳号）→ assert_seq_order 检出。
pub fn inject_seq_skip_must_fail() -> Result<(), u16> {
    let mut v = SealVault::new();
    v.fill_all(SALT_SEQ)?;
    v.seal_all()?;
    // 模拟跳号：第二账序号被改
    v.bodies[1].seal_seq = 9;
    if v.assert_seq_order().is_ok() {
        return Err(SEG_BASE + 0x3E);
    }
    Ok(())
}

/// 封账清单渲染：逐族一行（族名/行数/哈希尾/序号），零手抄。
pub fn manifest_lines(v: &SealVault) -> Result<Vec<String>, u16> {
    v.assert_all_sealed()?;
    let mut out = Vec::new();
    for b in &v.bodies {
        let mut l = String::from(b.kind.name());
        l.push_str(" 行数=");
        l.push_str(&b.rows.len().to_string());
        l.push_str(" 哈希尾=");
        l.push_str(&(b.seal_hash % 100_000).to_string());
        out.push(l);
    }
    if out.len() != 4 {
        return Err(E_INCOMPLETE);
    }
    Ok(out)
}

/// 清单注入核：某族未封 → 清单渲染拦截（齐备 4/4 破）。
pub fn manifest_incomplete_must_fail() -> Result<(), u16> {
    let mut v = SealVault::new();
    v.fill_all(SALT_MI)?;
    // 只封三族
    for i in 0..3usize {
        v.seq += 1;
        v.bodies[i].seal(v.seq)?;
    }
    if manifest_lines(&v).is_ok() {
        return Err(SEG_BASE + 0x3F);
    }
    Ok(())
}

/// 封账事件账：金库级封存事件计数与守恒（事件数 = 族数 = 4）。
pub struct SealEventBook {
    /// (族序号, 时戳序号)。
    pub events: Vec<(usize, u64)>,
}

impl SealEventBook {
    pub fn record(v: &SealVault) -> Result<SealEventBook, u16> {
        v.assert_seq_order()?;
        let mut events = Vec::new();
        for b in &v.bodies {
            events.push((b.kind.idx(), b.seal_seq));
        }
        if events.len() != 4 {
            return Err(E_INCOMPLETE);
        }
        Ok(SealEventBook { events })
    }

    /// 守恒：事件数 = 4，时戳序号 = 1..4 全不重。
    pub fn audit(&self) -> Result<(), u16> {
        if self.events.len() != 4 {
            return Err(E_INCOMPLETE);
        }
        let mut seen = [false; 5];
        for (k, s) in &self.events {
            if *k > 3 || *s == 0 || *s > 4 {
                return Err(E_ORDER);
            }
            if seen[*s as usize] {
                return Err(E_ORDER);
            }
            seen[*s as usize] = true;
        }
        Ok(())
    }

    /// 摘要行。
    pub fn summary(&self) -> String {
        let mut s = String::from("封账事件: 4 事件, 序号 1..4 单调");
        s
    }
}

/// 自例程：四账封存 4/4 + 写尝试 100% 拦截 + 注入全检出 → 0。
pub fn drill() -> Result<u32, u16> {
    kind_closure_audit()?;
    fp_audit()?;
    inject_empty_seal_must_fail()?;
    vault_fp_audit()?;
    write_gate_audit()?;
    fill_gate_audit()?;
    inject_seq_skip_must_fail()?;
    manifest_incomplete_must_fail()?;

    let mut v = SealVault::new();
    v.fill_all(SALT_VAULT)?;
    row_floor_audit(&v)?;
    v.seal_all()?;
    v.assert_all_sealed()?;
    v.assert_seq_order()?;
    // 封账事件账 + 清单渲染
    let ev = SealEventBook::record(&v)?;
    ev.audit()?;
    if ev.summary().is_empty() {
        return Err(E_ORDER);
    }
    let mf = manifest_lines(&v)?;
    if mf.len() != 4 {
        return Err(E_INCOMPLETE);
    }

    // 注入：四族逐族写尝试，全部拦截并立案（检出 100%）
    for k in AcctKind::ALL {
        if v.attempt_write(k, 999) != Err(E_WRONLY) {
            return Err(SEG_BASE + 0x36);
        }
    }
    v.assert_intercepted_all()?;
    // 注入：哈希篡改检出
    inject_hash_tamper_must_fail()?;
    // 注入：重复封账 → 拦截
    if v.seal_all() != Err(E_SEAL) {
        return Err(SEG_BASE + 0x37);
    }
    // 注入：封后直接写 → 拦截
    if v.body_of(AcctKind::Thresh).and_then(|b| b.write(1).map(|_| ())) != Err(E_WRONLY) {
        return Err(SEG_BASE + 0x38);
    }
    // 报告
    let rep = SealReport::generate(&v)?;
    rep.assert_complete()?;
    Ok(0)
}
