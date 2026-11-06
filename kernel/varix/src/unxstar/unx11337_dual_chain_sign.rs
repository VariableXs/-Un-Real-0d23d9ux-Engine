//! UNX-F11337 退出链双线联签（musl 线）（0xeb10 段）
//!
//! musl 的退出收尾有两条独立产线：`atexit` 回调线，和线程局部析构线。两条
//! 线各自有日志、各自有长度、各自可能被中途截断。若只验一条线，「回调全跑
//! 了但 TLS 析构漏了」这种缺陷会恰好落在两条线的缝隙里。
//!
//! 本模块因此要求**双线联签**：两条线都关闭、都有条目，并且「谁先关」这一
//! 次序字也进入签名。签名 = FNV 折叠（A 线摘要、B 线摘要、次序字），三样缺
//! 一不可。验证时不信任增量摘要，而是**从条目重新复算**两条线的摘要，再复
//! 算签名——这样任何对条目的篡改都会在验证时暴露，而不是继续沿用一份已经
//! 被污染的摘要。
//!
//! 「先关线」的判定只有一次：先关的那条线把 `order` 写成 1 或 2，后关的线
//! 不再改写。次序错了不要紧，但次序**必须存在**，否则签名无从谈起。
//!
//! 闸门次序：
//! - 入账：本线已关 → 类目为零 → 本线已满；
//! - 关线：本线已关 → 本线为空；
//! - 签名：已签 → A 线未关 → B 线未关 → 次序字缺失；
//! - 验签：未签 → 复算摘要不符 → 复算签名不符。
//!
//! 注意「已签」排在两条线检查之前：一旦签过，任何再次签名都只报 `ESIGNED`，
//! 因为此时再去检查线状态只会得到一份被签名动作固定过的快照，结论不稳定。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb10;

/// 类目为零。
pub const DS_EKIND: u32 = SEG_BASE + 0x01;
/// A 线已关闭。
pub const DS_EADONE: u32 = SEG_BASE + 0x02;
/// B 线已关闭。
pub const DS_EBDONE: u32 = SEG_BASE + 0x03;
/// A 线为空。
pub const DS_EAEMPTY: u32 = SEG_BASE + 0x04;
/// B 线为空。
pub const DS_EBEMPTY: u32 = SEG_BASE + 0x05;
/// A 线已满。
pub const DS_EAFULL: u32 = SEG_BASE + 0x06;
/// B 线已满。
pub const DS_EBFULL: u32 = SEG_BASE + 0x07;
/// 已签名。
pub const DS_ESIGNED: u32 = SEG_BASE + 0x08;
/// A 线尚未关闭。
pub const DS_EANOTDONE: u32 = SEG_BASE + 0x09;
/// B 线尚未关闭。
pub const DS_EBNOTDONE: u32 = SEG_BASE + 0x0a;
/// 未签名。
pub const DS_EUNSIGNED: u32 = SEG_BASE + 0x0b;
/// 摘要或签名不符。
pub const DS_EMISMATCH: u32 = SEG_BASE + 0x0c;
/// 账本失衡或重放不一致。
pub const DS_ESEAL: u32 = SEG_BASE + 0x0d;

/// 单线容量。
pub const CAP: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 次序：A 线先关。
pub const ORDER_A: u8 = 1;
/// 次序：B 线先关。
pub const ORDER_B: u8 = 2;

/// 操作码：A 线入账。
pub const OP_PUSHA: u8 = 1;
/// 操作码：B 线入账。
pub const OP_PUSHB: u8 = 2;
/// 操作码：关线。
pub const OP_CLOSE: u8 = 3;
/// 操作码：签名。
pub const OP_SIGN: u8 = 4;
/// 操作码：验签。
pub const OP_VERIFY: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 双线退出账。
#[derive(Clone, Copy, Debug)]
pub struct Dual {
    ak: [u32; CAP],
    av: [u32; CAP],
    bk: [u32; CAP],
    bv: [u32; CAP],
    an: usize,
    bn: usize,
    a_done: bool,
    b_done: bool,
    order: u8,
    signed: bool,
    signature: u64,
    verifies: u32,
}

impl Dual {
    /// 空双线账。
    pub const fn new() -> Dual {
        Dual {
            ak: [0; CAP],
            av: [0; CAP],
            bk: [0; CAP],
            bv: [0; CAP],
            an: 0,
            bn: 0,
            a_done: false,
            b_done: false,
            order: 0,
            signed: false,
            signature: 0,
            verifies: 0,
        }
    }

    /// A 线条数。
    pub fn a_len(&self) -> usize {
        self.an
    }

    /// B 线条数。
    pub fn b_len(&self) -> usize {
        self.bn
    }

    /// A 线是否已关。
    pub fn a_closed(&self) -> bool {
        self.a_done
    }

    /// B 线是否已关。
    pub fn b_closed(&self) -> bool {
        self.b_done
    }

    /// 次序字。
    pub fn order_code(&self) -> u8 {
        self.order
    }

    /// 已签名的两线总条数。
    pub fn signed_items(&self) -> usize {
        self.an + self.bn
    }

    /// 已关闭且有内容的线数。
    pub fn matched_lines(&self) -> u32 {
        let mut k = 0u32;
        if self.a_done && self.an > 0 {
            k += 1;
        }
        if self.b_done && self.bn > 0 {
            k += 1;
        }
        k
    }

    /// 签名低 32 位（用于判据取一个可比较的整数）。
    pub fn signature_low(&self) -> u32 {
        self.signature as u32
    }

    /// 验签成功次数。
    pub fn verifies(&self) -> u32 {
        self.verifies
    }

    /// 是否已签。
    pub fn is_signed(&self) -> bool {
        self.signed
    }

    /// 从条目复算 A 线摘要。
    fn recompute_a(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.an {
            if let Some(k) = self.ak.get(i) {
                h = fold(h, *k as u64);
            }
            if let Some(v) = self.av.get(i) {
                h = fold(h, *v as u64);
            }
            i += 1;
        }
        h
    }

    /// 从条目复算 B 线摘要。
    fn recompute_b(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.bn {
            if let Some(k) = self.bk.get(i) {
                h = fold(h, *k as u64);
            }
            if let Some(v) = self.bv.get(i) {
                h = fold(h, *v as u64);
            }
            i += 1;
        }
        h
    }

    /// 复算联合签名。
    fn recompute_signature(&self) -> u64 {
        let ha = self.recompute_a();
        let hb = self.recompute_b();
        let h = fold(fold(FNV_OFF, ha), hb);
        fold(h, self.order as u64)
    }

    /// A 线入账，返回 A 线条数。
    pub fn push_a(&mut self, kind: u32, val: u32) -> Result<usize, u32> {
        if self.a_done {
            return Err(DS_EADONE);
        }
        if kind == 0 {
            return Err(DS_EKIND);
        }
        if self.an >= CAP {
            return Err(DS_EAFULL);
        }
        if let Some(slot) = self.ak.get_mut(self.an) {
            *slot = kind;
        }
        if let Some(slot) = self.av.get_mut(self.an) {
            *slot = val;
        }
        self.an += 1;
        Ok(self.an)
    }

    /// B 线入账，返回 B 线条数。
    pub fn push_b(&mut self, kind: u32, val: u32) -> Result<usize, u32> {
        if self.b_done {
            return Err(DS_EBDONE);
        }
        if kind == 0 {
            return Err(DS_EKIND);
        }
        if self.bn >= CAP {
            return Err(DS_EBFULL);
        }
        if let Some(slot) = self.bk.get_mut(self.bn) {
            *slot = kind;
        }
        if let Some(slot) = self.bv.get_mut(self.bn) {
            *slot = val;
        }
        self.bn += 1;
        Ok(self.bn)
    }

    /// 关闭 A 线，返回条数。
    pub fn close_a(&mut self) -> Result<usize, u32> {
        if self.a_done {
            return Err(DS_EADONE);
        }
        if self.an == 0 {
            return Err(DS_EAEMPTY);
        }
        self.a_done = true;
        if !self.b_done {
            self.order = ORDER_A;
        }
        Ok(self.an)
    }

    /// 关闭 B 线，返回条数。
    pub fn close_b(&mut self) -> Result<usize, u32> {
        if self.b_done {
            return Err(DS_EBDONE);
        }
        if self.bn == 0 {
            return Err(DS_EBEMPTY);
        }
        self.b_done = true;
        if !self.a_done {
            self.order = ORDER_B;
        }
        Ok(self.bn)
    }

    /// 联合签名，返回两线总条数。
    ///
    /// 签名本身只存一份 64 位指纹；判据要的是「两线合计几条」，所以返回值
    /// 取条数——把「有没有签」与「签了几条」分开，前者看 `is_signed`。
    pub fn sign(&mut self) -> Result<usize, u32> {
        if self.signed {
            return Err(DS_ESIGNED);
        }
        if !self.a_done {
            return Err(DS_EANOTDONE);
        }
        if !self.b_done {
            return Err(DS_EBNOTDONE);
        }
        if self.order != ORDER_A && self.order != ORDER_B {
            return Err(DS_EANOTDONE);
        }
        self.signature = self.recompute_signature();
        self.signed = true;
        Ok(self.signed_items())
    }

    /// 验签，返回累计成功次数。
    ///
    /// 验签不复用增量摘要：两条线的摘要都从条目重算，签名也重算。这样条目
    /// 被篡改时 `recompute_signature` 与留存签名必然不等，验签失败可归因。
    pub fn verify(&mut self) -> Result<usize, u32> {
        if !self.signed {
            return Err(DS_EUNSIGNED);
        }
        let fresh = self.recompute_signature();
        if fresh != self.signature {
            return Err(DS_EMISMATCH);
        }
        self.verifies = self.verifies.saturating_add(1);
        Ok(self.verifies as usize)
    }

    /// A 线第 i 条的类目。
    pub fn a_kind(&self, i: usize) -> Result<u32, u32> {
        if i >= self.an {
            return Err(DS_EMISMATCH);
        }
        match self.ak.get(i) {
            Some(v) => Ok(*v),
            None => Err(DS_EMISMATCH),
        }
    }

    /// B 线第 i 条的值。
    pub fn b_val(&self, i: usize) -> Result<u32, u32> {
        if i >= self.bn {
            return Err(DS_EMISMATCH);
        }
        match self.bv.get(i) {
            Some(v) => Ok(*v),
            None => Err(DS_EMISMATCH),
        }
    }

    /// 篡改 A 线某条的值（只用于验证复算路径确实生效）。
    ///
    /// 刻意不改摘要、不改签名：如果验签还过，说明验签根本没复算。
    pub fn tamper_a(&mut self, i: usize, val: u32) -> Result<(), u32> {
        if i >= self.an {
            return Err(DS_EMISMATCH);
        }
        if let Some(slot) = self.av.get_mut(i) {
            *slot = val;
        }
        Ok(())
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(DS_ESEAL);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DS_ESEAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 2, 1, 3, 1, 2];
        const CODES: [u32; 6] = [
            DS_EKIND,
            DS_EUNSIGNED,
            DS_EADONE,
            DS_EBDONE,
            DS_ESIGNED,
            DS_EAEMPTY,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DS_ESEAL),
            };
            if s.slot as usize != i {
                return Err(DS_ESEAL);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DS_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DS_EMISMATCH);
                        }
                    }
                    None => return Err(DS_ESEAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DS_EMISMATCH);
                        }
                    }
                    None => return Err(DS_ESEAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DS_ESEAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DS_ESEAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(DS_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：两线入账 → 零类目 → 关线 → 联签 → 复验 → 终局与空线拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut d = Dual::new();

    let _ = l.register(cell(0, OP_PUSHA, d.push_a(1, 0x10)));
    let _ = l.register(cell(1, OP_PUSHA, d.push_a(2, 0x20)));
    let _ = l.register(cell(2, OP_PUSHB, d.push_b(3, 0x30)));
    let _ = l.register(cell(3, OP_PUSHA, d.push_a(0, 0)));
    let _ = l.register(cell(4, OP_CLOSE, d.close_a()));
    let _ = l.register(cell(5, OP_CLOSE, d.close_b()));
    let _ = l.register(cell(6, OP_SIGN, d.sign()));
    let _ = l.register(cell(7, OP_VERIFY, d.verify()));
    let _ = l.register(cell(8, OP_VERIFY, d.verify()));

    let mut n = Dual::new();
    let _ = l.register(cell(9, OP_VERIFY, n.verify()));

    let _ = l.register(cell(10, OP_PUSHA, d.push_a(4, 0x40)));
    let _ = l.register(cell(11, OP_PUSHB, d.push_b(4, 0x40)));
    let _ = l.register(cell(12, OP_SIGN, d.sign()));

    let mut e = Dual::new();
    let _ = l.register(cell(13, OP_CLOSE, e.close_a()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：双线都关、次序入签、两验递增。
    let mut a = Dual::new();
    let _ = a.push_a(1, 0xaa);
    let _ = a.push_b(2, 0xbb);
    let ca = a.close_a();
    let cb = a.close_b();
    let sg = a.sign();
    let v1 = a.verify();
    let v2 = a.verify();
    out[1] = ca == Ok(1)
        && cb == Ok(1)
        && a.order_code() == ORDER_A
        && a.matched_lines() == 2
        && sg == Ok(2)
        && a.signature_low() != 0
        && v1 == Ok(1)
        && v2 == Ok(2)
        && a.a_kind(0) == Ok(1)
        && a.b_val(0) == Ok(0xbb);

    // 二路：篡改条目后复算必不匹配，且成功计数不涨。
    let mut b = Dual::new();
    let _ = b.push_a(7, 1);
    let _ = b.push_b(8, 2);
    let _ = b.close_a();
    let _ = b.close_b();
    let _ = b.sign();
    let before = b.verifies();
    let tampered = b.tamper_a(0, 9);
    let bad = b.verify();
    out[2] = tampered == Ok(())
        && bad == Err(DS_EMISMATCH)
        && b.verifies() == before
        && b.tamper_a(9, 9) == Err(DS_EMISMATCH);

    // 三路：闸门（零类目 / 闭线 / 空线 / 未签 / 未关 / 重复签）。
    let mut c = Dual::new();
    let g1 = c.push_a(0, 1) == Err(DS_EKIND);
    let g2 = c.close_a() == Err(DS_EAEMPTY);
    let g3 = c.close_b() == Err(DS_EBEMPTY);
    let g4 = c.verify() == Err(DS_EUNSIGNED);
    let g5 = c.sign() == Err(DS_EANOTDONE);
    let _ = c.push_a(1, 1);
    let g6 = c.close_a() == Ok(1);
    let g7 = c.push_a(2, 2) == Err(DS_EADONE);
    let g8 = c.sign() == Err(DS_EBNOTDONE);
    let _ = c.push_b(3, 3);
    let g9 = c.close_b() == Ok(1);
    let g10 = c.close_b() == Err(DS_EBDONE);
    let g11 = c.sign() == Ok(2);
    let g12 = c.sign() == Err(DS_ESIGNED);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9 && g10 && g11 && g12;

    out
}

/// 十三项判据。
pub fn run_unx11337_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11337.dual_chain_sign");
    let l = build_ledger();

    s.add_dyn(
        "DS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DS2-两线入账计数",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DS3-零类目被拒",
        l.sample(3).map(|x| x.code) == Some(DS_EKIND),
        "static",
    );

    s.add(
        "DS4-关线返回条数",
        l.sample(4).map(|x| x.value) == Some(2)
            && l.sample(5).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DS5-联签返回合计条数",
        l.sample(6).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "DS6-首验通过",
        l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DS7-复验计数递增",
        l.sample(8).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DS8-未签验签被拒",
        l.sample(9).map(|x| x.code) == Some(DS_EUNSIGNED),
        "static",
    );

    s.add(
        "DS9-闭线后入账被拒",
        l.sample(10).map(|x| x.code) == Some(DS_EADONE)
            && l.sample(11).map(|x| x.code) == Some(DS_EBDONE),
        "static",
    );

    s.add(
        "DS10-重复签名被拒",
        l.sample(12).map(|x| x.code) == Some(DS_ESIGNED),
        "static",
    );

    s.add(
        "DS11-空线不能关闭",
        l.sample(13).map(|x| x.code) == Some(DS_EAEMPTY),
        "static",
    );

    s.add(
        "DS12-次序字与双线匹配",
        {
            let mut d = Dual::new();
            let _ = d.push_b(5, 1);
            let _ = d.close_b();
            let _ = d.push_a(6, 2);
            let _ = d.close_a();
            let sg = d.sign();
            sg == Ok(2)
                && d.order_code() == ORDER_B
                && d.matched_lines() == 2
                && d.signature_low() != 0
                && d.verify() == Ok(1)
                && d.a_len() == 1
                && d.b_len() == 1
        },
        "static",
    );

    s.add(
        "DS13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11337_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn sign_and_verify() {
        let mut d = Dual::new();
        assert_eq!(d.push_a(1, 1), Ok(1));
        assert_eq!(d.push_b(2, 2), Ok(1));
        assert_eq!(d.close_a(), Ok(1));
        assert_eq!(d.close_b(), Ok(1));
        assert_eq!(d.sign(), Ok(2));
        assert!(d.is_signed());
        assert_eq!(d.verify(), Ok(1));
        assert_eq!(d.verify(), Ok(2));
        assert_eq!(d.sign(), Err(DS_ESIGNED));
    }

    #[test]
    fn tamper_breaks_verification() {
        let mut d = Dual::new();
        let _ = d.push_a(1, 1);
        let _ = d.push_b(2, 2);
        let _ = d.close_a();
        let _ = d.close_b();
        let _ = d.sign();
        assert_eq!(d.tamper_a(0, 7), Ok(()));
        assert_eq!(d.verify(), Err(DS_EMISMATCH));
        assert_eq!(d.verifies(), 0);
        assert_eq!(d.tamper_a(5, 1), Err(DS_EMISMATCH));
    }

    #[test]
    fn order_is_recorded_once() {
        let mut d = Dual::new();
        let _ = d.push_a(1, 1);
        let _ = d.push_b(2, 2);
        assert_eq!(d.close_b(), Ok(1));
        assert_eq!(d.order_code(), ORDER_B);
        assert_eq!(d.close_a(), Ok(1));
        assert_eq!(d.order_code(), ORDER_B);
    }

    #[test]
    fn closed_lines_reject_pushes() {
        let mut d = Dual::new();
        let _ = d.push_a(1, 1);
        let _ = d.close_a();
        assert_eq!(d.push_a(2, 2), Err(DS_EADONE));
        assert_eq!(d.push_b(0, 2), Err(DS_EKIND));
        let _ = d.push_b(3, 3);
        let _ = d.close_b();
        assert_eq!(d.push_b(4, 4), Err(DS_EBDONE));
        assert_eq!(d.close_a(), Err(DS_EADONE));
    }

    #[test]
    fn sign_needs_both_lines() {
        let mut d = Dual::new();
        let _ = d.push_a(1, 1);
        assert_eq!(d.sign(), Err(DS_EANOTDONE));
        assert_eq!(d.close_a(), Ok(1));
        assert_eq!(d.sign(), Err(DS_EBNOTDONE));
        assert_eq!(d.verify(), Err(DS_EUNSIGNED));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn inject_paths_agree() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
