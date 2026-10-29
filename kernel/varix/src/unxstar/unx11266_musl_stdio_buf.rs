//! UNX-F11266 musl stdio 静态路径消费（0x1f420 段）
//! 纯功能模型：三态缓冲（无缓冲/行缓冲/全缓冲）的 write 时序——printf 不带
//! 换行、puts 自带换行、fwrite 按块；逐次写入推演刷出序列与守恒账，
//! 三种调用面在同一负载下必须给出同一刷出时序。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f420 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f420;
/// 缓冲模式越出 0..=2 闭集。
pub const SB_MODE: u32 = SEG_BASE + 0x01;
/// 调用面越出 1..=3 闭集。
pub const SB_KIND: u32 = SEG_BASE + 0x02;
/// 写入长度为 0 或超缓冲容量。
pub const SB_BYTES: u32 = SEG_BASE + 0x03;
/// 空缓冲刷出（零长刷）或重复刷。
pub const SB_EMPTY: u32 = SEG_BASE + 0x04;
/// 刷出记录表已满。
pub const SB_CAP: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const SB_FROZEN: u32 = SEG_BASE + 0x06;
/// 未刷净即切模式。
pub const SB_ORDER: u32 = SEG_BASE + 0x07;

/// 三态缓冲闭集。
pub const M_NONE: u8 = 0;
pub const M_LINE: u8 = 1;
pub const M_FULL: u8 = 2;
/// 调用面闭集。
pub const K_PRINTF: u8 = 1;
pub const K_PUTS: u8 = 2;
pub const K_FWRITE: u8 = 3;
/// 缓冲容量（BUFSIZ 量级）。
pub const BUF_CAP: u32 = 1024;
/// 刷出记录定容。
pub const FLUSH_CAP: usize = 64;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_66;

/// 一次刷出的记录：字节数 + 触发调用面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flush {
    pub bytes: u32,
    pub kind: u8,
}

impl Flush {
    pub fn render(&self) -> String {
        format!("F{}b/k{}", self.bytes, self.kind)
    }
}

/// stdio 缓冲账：模式 + 待刷字节 + 刷出序列 + 守恒对。
#[derive(Clone, Debug)]
pub struct Stdio {
    pub mode: u8,
    pub pending: u32,
    pub total_in: u32,
    pub total_out: u32,
    pub last_kind: u8,
    pub flushes: Vec<Flush>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Stdio {
    pub fn new() -> Stdio {
        Stdio {
            mode: M_FULL,
            pending: 0,
            total_in: 0,
            total_out: 0,
            last_kind: 0,
            flushes: Vec::new(),
            accepts: 0,
            rejects: 0,
            frozen: false,
            root: None,
        }
    }

    /// 切模式：必须已刷净（未刷净即切按次序错入账）。
    pub fn set_mode(&mut self, m: u8) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_FROZEN);
        }
        if m > M_FULL {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_MODE);
        }
        if self.pending != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_ORDER);
        }
        self.mode = m;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(())
    }

    /// 写一笔。判定序：封册 → 调用面 → 长度 → 入缓冲 → 按模式判刷。
    pub fn write(&mut self, kind: u8, n: u32) -> Result<u32, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_FROZEN);
        }
        if kind < K_PRINTF || kind > K_FWRITE {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_KIND);
        }
        if n == 0 || n > BUF_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_BYTES);
        }
        // puts 自带换行，故有效负载 +1。
        let payload = n.wrapping_add(if kind == K_PUTS { 1 } else { 0 });
        self.total_in = self.total_in.wrapping_add(payload);
        self.pending = self.pending.wrapping_add(payload);
        self.last_kind = kind;
        self.accepts = self.accepts.wrapping_add(1);
        let need = match self.mode {
            M_NONE => true,
            M_LINE => kind == K_PUTS || self.pending >= BUF_CAP,
            M_FULL => self.pending >= BUF_CAP,
            _ => true,
        };
        if need {
            let _ = self.flush();
        }
        Ok(self.total_in)
    }

    /// 刷出待刷字节。空刷按零长刷入账（不静默）。
    pub fn flush(&mut self) -> Result<u32, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_FROZEN);
        }
        if self.pending == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_EMPTY);
        }
        if self.flushes.len() >= FLUSH_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(SB_CAP);
        }
        let b = self.pending;
        self.flushes.push(Flush { bytes: b, kind: self.last_kind });
        self.total_out = self.total_out.wrapping_add(b);
        self.pending = 0;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(b)
    }

    /// 字节守恒：入 = 出 + 待刷。
    pub fn conserved(&self) -> bool {
        self.total_in == self.total_out.wrapping_add(self.pending)
    }

    /// 刷出字节序列。
    pub fn seq(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.flushes.len() {
            if let Some(f) = self.flushes.get(k) {
                v.push(f.bytes);
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// FNV-1a 链指纹：模式/待刷/总量/刷出序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.mode as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.pending as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.total_in as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.total_out as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.flushes.len() {
            if let Some(f) = self.flushes.get(k) {
                h = (h ^ f.bytes as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ f.kind as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 同一负载在三态缓冲下的刷出时序样本。
pub fn script(mode: u8, kind: u8) -> Stdio {
    let mut s = Stdio::new();
    let _ = s.set_mode(mode);
    let _ = s.write(kind, 100);
    let _ = s.write(kind, 200);
    let _ = s.write(kind, 300);
    s
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) 模式越闭集
    let mut a = Stdio::new();
    got[0] = a.set_mode(9).err() == Some(SB_MODE);
    // (b) 重复刷（空刷）
    let mut b = Stdio::new();
    let _ = b.set_mode(M_NONE);
    let _ = b.write(K_PRINTF, 10);
    got[1] = b.flush().err() == Some(SB_EMPTY);
    // (c) 形状违规：零长写入
    let mut c = Stdio::new();
    got[2] = c.write(K_PRINTF, 0).err() == Some(SB_BYTES);
    // (d) 封册后写账
    let mut d = script(M_NONE, K_PRINTF);
    let _ = d.freeze();
    got[3] = d.write(K_PRINTF, 10).err() == Some(SB_FROZEN);
    got
}

/// stdio 三态缓冲判据。
pub fn run_unx11266_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11266");

    // X1 无缓冲：逐笔即刷
    let x1 = {
        let b = script(M_NONE, K_PRINTF);
        b.flushes.len() == 3 && b.pending == 0 && b.conserved() && b.total_out == 600
    };
    s.add("X1-无缓冲即刷", x1, "三笔写出三次刷出，守恒成立");

    // X2 行缓冲：无换行不刷，puts 触发整行刷
    let x2 = {
        let b = script(M_LINE, K_PRINTF);
        let quiet = b.flushes.is_empty() && b.pending == 600 && b.conserved();
        let mut c = Stdio::new();
        let _ = c.set_mode(M_LINE);
        let _ = c.write(K_PRINTF, 100);
        let _ = c.write(K_PRINTF, 200);
        let _ = c.write(K_PRINTF, 300);
        let held = c.flushes.is_empty() && c.pending == 600;
        let _ = c.write(K_PUTS, 1);
        quiet && held && c.flushes.len() == 1 && c.conserved() && c.total_out == 602
    };
    s.add("X2-行缓冲时序", x2, "printf 攒行、puts 刷整行");

    // X3 全缓冲：攒满容量才刷
    let x3 = {
        let mut b = Stdio::new();
        let _ = b.write(K_PRINTF, 500);
        let mid = b.flushes.is_empty();
        let _ = b.write(K_PRINTF, 600);
        mid && b.flushes.len() == 1 && b.conserved() && b.total_out == 1100
    };
    s.add("X3-全缓冲阈值", x3, "500 不刷、1100 触发一次刷");

    // X4 三面同码：printf 与 fwrite 逐字节同序
    let x4 = {
        let a = script(M_NONE, K_PRINTF);
        let b = script(M_NONE, K_FWRITE);
        a.seq() == b.seq()
    };
    s.add("X4-两面具同码", x4, "printf 与 fwrite 刷出序列逐项一致");

    // X5 puts 换行增量：每笔恰多 1 字节
    let x5 = {
        let a = script(M_NONE, K_PRINTF);
        let b = script(M_NONE, K_PUTS);
        let sa = a.seq();
        let sb = b.seq();
        sa.len() == sb.len()
            && sa.iter().zip(sb.iter()).all(|(x, y)| y.wrapping_sub(*x) == 1)
    };
    s.add("X5-换行增量", x5, "puts 每笔比 printf 恰多 1 字节");

    // X6 模式门禁：越集与未刷净切换
    let x6 = {
        let mut a = Stdio::new();
        let over = a.set_mode(9).err() == Some(SB_MODE);
        let _ = a.set_mode(M_LINE);
        let _ = a.write(K_PRINTF, 10);
        let dirty = a.set_mode(M_FULL).err() == Some(SB_ORDER);
        over && dirty
    };
    s.add("X6-模式门禁", x6, "越集与未刷净切换均拒");

    // X7 调用面闭集
    let x7 = {
        let mut a = Stdio::new();
        let low = a.write(0, 10).err() == Some(SB_KIND);
        let high = a.write(4, 10).err() == Some(SB_KIND);
        low && high
    };
    s.add("X7-调用面闭集", x7, "调用面 0 与 4 均越闭集");

    // X8 字节门禁
    let x8 = {
        let mut a = Stdio::new();
        let zero = a.write(K_PRINTF, 0).err() == Some(SB_BYTES);
        let over = a.write(K_PRINTF, BUF_CAP.wrapping_add(1)).err() == Some(SB_BYTES);
        zero && over
    };
    s.add("X8-字节门禁", x8, "零长与超容量写入均拒");

    // X9 空刷入账
    let x9 = {
        let mut a = Stdio::new();
        let _ = a.set_mode(M_NONE);
        let _ = a.write(K_PRINTF, 8);
        a.flush().err() == Some(SB_EMPTY)
    };
    s.add("X9-空刷入账", x9, "零长刷出报空刷");

    // X10 刷出表定容
    let x10 = {
        let mut b = Stdio::new();
        let _ = b.set_mode(M_LINE);
        let mut i = 0usize;
        while i < FLUSH_CAP {
            let _ = b.write(K_PUTS, 1);
            i = i.wrapping_add(1);
        }
        let full = b.flushes.len() == FLUSH_CAP;
        let _ = b.write(K_PUTS, 1);
        full && b.flush().err() == Some(SB_CAP)
    };
    s.add("X10-刷出定容", x10, "64 次后第 65 次刷出报满");

    // X11 指纹确定
    let x11 = {
        let b = script(M_LINE, K_PUTS);
        let a = b.fold();
        a == b.fold() && a != 0
    };
    s.add("X11-指纹确定", x11, "同账两次指纹一致且非零");

    // X12 封册幂等
    let x12 = {
        let mut b = script(M_NONE, K_FWRITE);
        let r1 = b.freeze();
        let r2 = b.freeze();
        let live = b.fold();
        b.flushes.clear();
        let r3 = b.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != b.fold()
    };
    s.add("X12-封册幂等", x12, "二次封册回原根，账变不改根");

    // X13 封册拒写与刷出明细
    let (x13, line13) = {
        let mut b = script(M_NONE, K_PRINTF);
        let _ = b.freeze();
        let w = b.write(K_PRINTF, 10).err() == Some(SB_FROZEN);
        let f = b.flush().err() == Some(SB_FROZEN);
        let line = format!("w={} f={} seq={:?}", w, f, b.seq());
        (w && f && b.conserved(), line)
    };
    s.add_dyn("X13-封册明细", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11266_flow() {
        let mut b = script(M_LINE, K_PUTS);
        assert_eq!(b.flushes.len(), 3);
        assert_eq!(b.total_in, 603);
        assert_eq!(b.total_out, 603);
        assert!(b.conserved());
        assert_eq!(b.accepts, 7);
        assert_eq!(b.rejects, 0);
        let r1 = b.freeze();
        let r2 = b.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(b.root == r1);
        assert!(b.write(K_PRINTF, 10).err() == Some(SB_FROZEN));
    }

    #[test]
    fn unx11266_gates() {
        let mut b = Stdio::new();
        assert!(b.set_mode(M_NONE).is_ok());
        assert!(b.set_mode(9).err() == Some(SB_MODE));
        assert!(b.write(K_PRINTF, 100).is_ok());
        assert!(b.write(4, 10).err() == Some(SB_KIND));
        assert!(b.write(K_PRINTF, 0).err() == Some(SB_BYTES));
        assert!(b.write(K_PRINTF, 2048).err() == Some(SB_BYTES));
        assert!(b.flush().err() == Some(SB_EMPTY));
        assert!(b.set_mode(M_LINE).is_ok());
        assert!(b.write(K_PRINTF, 200).is_ok());
        assert!(b.set_mode(M_FULL).err() == Some(SB_ORDER));
        assert!(b.flush().is_ok());
        assert!(b.set_mode(M_FULL).is_ok());
        assert!(b.freeze().is_some());
        assert!(b.write(K_PRINTF, 10).err() == Some(SB_FROZEN));
        assert!(b.flush().err() == Some(SB_FROZEN));
        assert!(b.set_mode(M_NONE).err() == Some(SB_FROZEN));
        assert_eq!(b.accepts, 7);
        assert_eq!(b.rejects, 9);
    }

    #[test]
    fn unx11266_checks_green() {
        assert!(run_unx11266_checks().all_passed(), "UNX-F11266 判据未全绿");
    }
}
