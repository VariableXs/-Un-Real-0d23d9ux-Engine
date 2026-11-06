//! UNX-F11444 挂载标志解析（0x20b80 段）
//!
//! 挂载选项是从用户空间传入的一串逗号分隔的短词，内核逐词把它折叠成一组布尔
//! 标志。这个模型强调三条规则：每个词要么被识别，要么整串被拒绝；同一个语义
//! 的正反两个词靠后出现者获胜；空词（连续逗号、结尾逗号、整串为空）不是“忽略
//! 即可”，而是语法错误。把拒绝做成全串原子操作，是为了避免半个选项已经生效、
//! 半个选项报错的中间状态。
//!
//! 七组开关分别是只读、setuid、设备节点、可执行映射、同步写、relatime 与
//! noatime。最后两组互斥，`defaults` 则把所有位清零。解析器不认识任何取值型
//! 选项，因此出现等号也按未知词处理。重挂载走的是同一个折叠函数，只是起点是
//! 旧的标志字，而不是零。
//!
//! 账本把起点、选项串、结果标志或错误码固定在一起。审计会从起点重放选项串，
//! 验证保存的结果仍然成立，并核对成功数与失败数与逐行记录相等。封印把全部
//! 行的字节、长度与结果压进 FNV-1a，冻结之后任何登记都被拒绝，重放则重建一份
//! 新账本比较签名，因此模型既检查解析也检查记录本身没有漂移。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b80;
/// 编号非法。
pub const MF_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const MF_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const MF_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const MF_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const MF_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const MF_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const MF_EBAL: u32 = SEG_BASE + 0x07;
/// 未知选项。
pub const MF_EINVAL: u32 = SEG_BASE + 0x08;
/// 选项串过长。
pub const MF_ENAMETOOLONG: u32 = SEG_BASE + 0x09;
/// 票据不一致。
pub const MF_ESEAL: u32 = SEG_BASE + 0x0a;

/// 只读。
pub const F_RO: u32 = 0x0001;
/// 禁止 setuid。
pub const F_NOSUID: u32 = 0x0002;
/// 禁止设备节点。
pub const F_NODEV: u32 = 0x0004;
/// 禁止执行映射。
pub const F_NOEXEC: u32 = 0x0008;
/// 同步写。
pub const F_SYNC: u32 = 0x0010;
/// 相对访问时间。
pub const F_RELATIME: u32 = 0x0020;
/// 不更新访问时间。
pub const F_NOATIME: u32 = 0x0040;

/// 所有已知位。
pub const F_ALL: u32 = 0x007f;
/// 选项串容量。
pub const SPEC_CAP: usize = 48;
/// 账本格数。
pub const ROWS: usize = 14;

/// 一格输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub base: u32,
    pub spec: [u8; SPEC_CAP],
    pub len: u8,
    pub out: u32,
    pub code: u16,
    pub ok: bool,
}

impl Row {
    pub fn bytes(&self) -> &[u8] {
        let n = self.len as usize;
        if n > SPEC_CAP {
            return &[];
        }
        match self.spec.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

/// 把一个词折叠进标志字。返回 false 表示未知词。
fn word(base: u32, w: &[u8]) -> Option<u32> {
    if same(w, b"ro") {
        return Some(base | F_RO);
    }
    if same(w, b"rw") {
        return Some(base & !F_RO);
    }
    if same(w, b"nosuid") {
        return Some(base | F_NOSUID);
    }
    if same(w, b"suid") {
        return Some(base & !F_NOSUID);
    }
    if same(w, b"nodev") {
        return Some(base | F_NODEV);
    }
    if same(w, b"dev") {
        return Some(base & !F_NODEV);
    }
    if same(w, b"noexec") {
        return Some(base | F_NOEXEC);
    }
    if same(w, b"exec") {
        return Some(base & !F_NOEXEC);
    }
    if same(w, b"sync") {
        return Some(base | F_SYNC);
    }
    if same(w, b"async") {
        return Some(base & !F_SYNC);
    }
    if same(w, b"relatime") {
        return Some((base & !F_NOATIME) | F_RELATIME);
    }
    if same(w, b"noatime") {
        return Some((base & !F_RELATIME) | F_NOATIME);
    }
    if same(w, b"defaults") {
        return Some(0);
    }
    None
}

/// 折叠整串选项。空串与空词都拒绝，未知词整串拒绝。
pub fn parse(base: u32, spec: &[u8]) -> Result<u32, u32> {
    if spec.is_empty() {
        return Err(MF_EINVAL);
    }
    if spec.len() > SPEC_CAP {
        return Err(MF_ENAMETOOLONG);
    }
    let mut cur = base & F_ALL;
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= spec.len() {
        let at_end = i == spec.len();
        let comma = match spec.get(i) {
            Some(v) => *v == b',',
            None => at_end,
        };
        if at_end || comma {
            if i == start {
                return Err(MF_EINVAL);
            }
            let w = match spec.get(start..i) {
                Some(v) => v,
                None => return Err(MF_EBAL),
            };
            cur = match word(cur, w) {
                Some(v) => v,
                None => return Err(MF_EINVAL),
            };
            start = i + 1;
        }
        i += 1;
    }
    Ok(cur)
}

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 账本。
pub struct Ledger {
    rows: Vec<Row>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { rows: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, base: u32, spec: &[u8], out: u32, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(MF_EFROZEN);
        }
        if (slot as usize) >= ROWS {
            self.rejects += 1;
            return Err(MF_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(MF_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(MF_EORDER);
        }
        let r = parse(base, spec);
        let mut row = Row { base, spec: [0; SPEC_CAP], len: 0, out: 0, code: 0, ok };
        row.len = put(&mut row.spec, spec);
        match r {
            Ok(v) => {
                if !ok {
                    self.rejects += 1;
                    return Err(MF_EMISMATCH);
                }
                if v != out {
                    self.rejects += 1;
                    return Err(MF_EMISMATCH);
                }
                row.out = v;
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(MF_EMISMATCH);
                }
                row.code = (e & 0xffff) as u16;
            }
        }
        self.rows.push(row);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, i: usize) -> Option<Row> {
        match self.rows.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.rows.len() - self.ok_count()
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.is_empty() {
            return Err(MF_EBAL);
        }
        let mut ok = 0usize;
        let mut bad = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MF_ENOBS),
            };
            if r.len as usize > SPEC_CAP || (r.len as usize) != r.bytes().len() {
                return Err(MF_EBAL);
            }
            let got = parse(r.base, r.bytes());
            match got {
                Ok(v) => {
                    if v != r.out || r.code != 0 || !r.ok {
                        return Err(MF_EBAL);
                    }
                    ok += 1;
                }
                Err(e) => {
                    if r.ok || r.out != 0 || r.code != (e & 0xffff) as u16 {
                        return Err(MF_EBAL);
                    }
                    bad += 1;
                }
            }
            i += 1;
        }
        if ok != self.ok_count() || bad != self.err_count() {
            return Err(MF_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.base as u64);
                h = fold(h, r.len as u64);
                let mut k = 0usize;
                while k < r.len as usize {
                    if let Some(b) = r.spec.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                h = fold(h, r.out as u64);
                h = fold(h, r.code as u64);
                h = fold(h, if r.ok { 1 } else { 0 });
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(MF_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(MF_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, 0, b"rw", 0, true);
    let _ = l.register(1, 0, b"ro", F_RO, true);
    let _ = l.register(2, 0, b"ro,rw", 0, true);
    let _ = l.register(3, 0, b"nosuid,nodev,noexec", F_NOSUID | F_NODEV | F_NOEXEC, true);
    let _ = l.register(4, 0, b"ro,nosuid,suid", F_RO, true);
    let _ = l.register(5, 0, b"sync,async", 0, true);
    let _ = l.register(6, 0, b"relatime,noatime", F_NOATIME, true);
    let _ = l.register(7, 0, b"noatime,relatime", F_RELATIME, true);
    let _ = l.register(8, F_ALL, b"defaults", 0, true);
    let _ = l.register(9, 0, b"ro,bogus", 0, false);
    let _ = l.register(10, 0, b"ro,,rw", 0, false);
    let _ = l.register(11, 0, b"", 0, false);
    let _ = l.register(12, F_RO, b"rw,relatime", F_RELATIME, true);
    let _ = l.register(13, 0, b"nosuid,ro,nosuid", F_RO | F_NOSUID, true);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == ROWS && l.ok_count() == 11 && l.err_count() == 3 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    out[1] = parse(0, b"ro,nosuid") == Ok(F_RO | F_NOSUID)
        && parse(F_RO, b"rw") == Ok(0)
        && parse(0, b"suid,nosuid") == Ok(F_NOSUID);

    out[2] = parse(0, b"ro,bogus") == Err(MF_EINVAL)
        && parse(0, b"ro,,rw") == Err(MF_EINVAL)
        && parse(0, b"") == Err(MF_EINVAL)
        && parse(0, b"exec,noexec") == Ok(F_NOEXEC);

    out[3] = parse(F_NOATIME, b"relatime") == Ok(F_RELATIME)
        && parse(F_ALL, b"defaults,ro") == Ok(F_RO)
        && parse(F_RO, b"rw,noexec") == Ok(F_NOEXEC);
    out
}

/// 十三判据。
pub fn run_unx11444_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11444.mount_flags");
    let l = build_ledger();
    s.add("MF1-十四格", l.len() == ROWS, "static");
    s.add_dyn("MF2-十一成三拒", l.ok_count() == 11 && l.err_count() == 3 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("MF3-rw与ro", l.row(0).map(|r| r.out) == Some(0) && l.row(1).map(|r| r.out) == Some(F_RO), "static");
    s.add("MF4-后者获胜", l.row(2).map(|r| r.out) == Some(0) && l.row(4).map(|r| r.out) == Some(F_RO), "static");
    s.add("MF5-三禁", l.row(3).map(|r| r.out) == Some(F_NOSUID | F_NODEV | F_NOEXEC), "static");
    s.add("MF6-同步异步", l.row(5).map(|r| r.out) == Some(0), "static");
    s.add("MF7-两时间互斥", l.row(6).map(|r| r.out) == Some(F_NOATIME) && l.row(7).map(|r| r.out) == Some(F_RELATIME), "static");
    s.add("MF8-defaults清零", l.row(8).map(|r| r.out) == Some(0), "static");
    s.add("MF9-三种空词", l.row(9).map(|r| r.code) == Some((MF_EINVAL & 0xffff) as u16) && l.row(10).map(|r| r.code) == Some((MF_EINVAL & 0xffff) as u16) && l.row(11).map(|r| r.code) == Some((MF_EINVAL & 0xffff) as u16), "static");
    s.add("MF10-重挂载", l.row(12).map(|r| r.out) == Some(F_RELATIME) && l.row(13).map(|r| r.out) == Some(F_RO | F_NOSUID), "static");
    s.add("MF11-串长上限", parse(0, b"0123456789012345678901234567890123456789012345678") == Err(MF_ENAMETOOLONG) && l.row(3).map(|r| r.len as usize) == Some(19), "static");
    s.add("MF12-计数守恒", l.rejects() == 0 && l.ok_count() + l.err_count() == l.len(), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(MF_EFROZEN);
    let inj = inject_four();
    s.add_dyn("MF13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only() {
        assert_eq!(parse(0, b"ro"), Ok(F_RO));
        assert_eq!(parse(F_RO, b"rw"), Ok(0));
    }

    #[test]
    fn last_wins() {
        assert_eq!(parse(0, b"ro,nosuid,suid"), Ok(F_RO));
        assert_eq!(parse(0, b"noatime,relatime"), Ok(F_RELATIME));
    }

    #[test]
    fn empty_parts_rejected() {
        assert_eq!(parse(0, b""), Err(MF_EINVAL));
        assert_eq!(parse(0, b"ro,"), Err(MF_EINVAL));
        assert_eq!(parse(0, b",ro"), Err(MF_EINVAL));
    }

    #[test]
    fn unknown_rejected() {
        assert_eq!(parse(0, b"ro,bogus"), Err(MF_EINVAL));
        assert_eq!(parse(0, b"size=64k"), Err(MF_EINVAL));
    }

    #[test]
    fn defaults_clear() {
        assert_eq!(parse(F_ALL, b"defaults"), Ok(0));
        assert_eq!(parse(F_ALL, b"defaults,ro"), Ok(F_RO));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 11);
        assert_eq!(l.err_count(), 3);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn inject_and_checks() {
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11444_checks().failed_count(), 0);
    }
}
