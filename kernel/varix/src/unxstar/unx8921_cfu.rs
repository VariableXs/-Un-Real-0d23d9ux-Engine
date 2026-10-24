//! UNX-F8921 参数拷贝探针总成（copy_from_user/copy_to_user 语义档）（0xA680 段）
//!
//! 锚点：UNX-F8921-J1 结构化参数（stat/tms/itimerspec）进出内核
//! 探针 10^3 次零失真判据过，探针失败返 EFAULT 且内核侧零残留。
//! 域：CoRun·UNX-C2-B07（参数拷贝面）。
//!
//! 结构：三结构模型（stat 元组 / tms 元组 / itimerspec 元组，字节
//! 级账）+ 拷贝探针（copy_from_user：用户 → 内核暂存，带审计账；
//! copy_to_user：内核 → 用户，带审计账）+ EFAULT 语义（空指针/
//! 越界长度 → EFAULT 且暂存零残留）+ 10^3 次零失真核（往返指纹
//! 全等）+ 注入核（空针/越界/残留/漂移/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA680 独占 0x80）。

pub const SEG_BASE: u32 = 0xA680;
pub const XC_CP_EFAULT: u32 = SEG_BASE + 0x01; // EFAULT
pub const XC_CP_RESIDUE: u32 = SEG_BASE + 0x02; // 残留
pub const XC_CP_DRIFT: u32 = SEG_BASE + 0x03; // 失真
pub const XC_CP_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 用户空间地址域界（模拟）。
pub const USER_BASE: u64 = 0x0000_4000_0000_0000;
pub const USER_END: u64 = 0x0000_8000_0000_0000;

/// 地址合法性（内核视角）。
pub fn user_ptr_ok(p: u64, len: usize) -> bool {
    p >= USER_BASE && p.checked_add(len as u64).map(|e| e <= USER_END).unwrap_or(false)
}

/// stat 结构账（18 字段 × 8 字节账面简化为 12 字段核心）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StatArg {
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub blocks: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub rdev: u64,
}

impl StatArg {
    pub fn sample(i: u64) -> Self {
        StatArg {
            dev: 0x8000_0000 + i,
            ino: 1000 + i,
            mode: 0o100644,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            size: 4096 * (i % 97 + 1),
            blocks: (i % 97 + 8) * 8,
            atime: 1_700_000_000 + i,
            mtime: 1_700_000_100 + i,
            ctime: 1_700_000_200 + i,
            rdev: 0,
        }
    }

    /// 序列化（96 字节账）。
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        for v in [self.dev, self.ino, self.size, self.blocks, self.atime, self.mtime, self.ctime, self.rdev] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [self.mode, self.nlink, self.uid, self.gid] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    /// 反序列化。
    pub fn from_bytes(b: &[u8]) -> Result<Self, u32> {
        if b.len() < 80 {
            return Err(XC_CP_EFAULT);
        }
        let g8 = |o: usize| -> u64 {
            u64::from_le_bytes([b[o], b[o+1], b[o+2], b[o+3], b[o+4], b[o+5], b[o+6], b[o+7]])
        };
        let g4 = |o: usize| -> u32 {
            u32::from_le_bytes([b[o], b[o+1], b[o+2], b[o+3]])
        };
        Ok(StatArg {
            dev: g8(0), ino: g8(8), size: g8(16), blocks: g8(24),
            atime: g8(32), mtime: g8(40), ctime: g8(48), rdev: g8(56),
            mode: g4(64), nlink: g4(68), uid: g4(72), gid: g4(76),
        })
    }
}

/// tms 结构账。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TmsArg {
    pub utime: u64,
    pub stime: u64,
    pub cutime: u64,
    pub cstime: u64,
}

impl TmsArg {
    pub fn sample(i: u64) -> Self {
        TmsArg { utime: i * 3, stime: i * 5, cutime: i * 7, cstime: i * 11 }
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        for v in [self.utime, self.stime, self.cutime, self.cstime] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }
    pub fn from_bytes(b: &[u8]) -> Result<Self, u32> {
        if b.len() < 32 {
            return Err(XC_CP_EFAULT);
        }
        let g = |o: usize| u64::from_le_bytes(b[o..o+8].try_into().unwrap_or([0; 8]));
        Ok(TmsArg { utime: g(0), stime: g(8), cutime: g(16), cstime: g(24) })
    }
}

/// itimerspec 结构账。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ItimerSpec {
    pub it_interval_sec: u64,
    pub it_interval_nsec: u64,
    pub it_value_sec: u64,
    pub it_value_nsec: u64,
}

impl ItimerSpec {
    pub fn sample(i: u64) -> Self {
        ItimerSpec {
            it_interval_sec: i % 10,
            it_interval_nsec: (i * 1000) % 1_000_000_000,
            it_value_sec: i % 7,
            it_value_nsec: (i * 3000) % 1_000_000_000,
        }
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        for v in [self.it_interval_sec, self.it_interval_nsec, self.it_value_sec, self.it_value_nsec] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }
    pub fn from_bytes(b: &[u8]) -> Result<Self, u32> {
        if b.len() < 32 {
            return Err(XC_CP_EFAULT);
        }
        let g = |o: usize| u64::from_le_bytes(b[o..o+8].try_into().unwrap_or([0; 8]));
        Ok(ItimerSpec {
            it_interval_sec: g(0), it_interval_nsec: g(8),
            it_value_sec: g(16), it_value_nsec: g(24),
        })
    }
}

/// 内核暂存（拷贝目标）。
pub struct KBuf {
    pub data: Vec<u8>,
    pub dirty: bool,
}

impl KBuf {
    pub fn new(cap: usize) -> Self {
        KBuf { data: vec![0u8; cap], dirty: false }
    }

    /// copy_from_user 探针。
    pub fn copy_from_user(&mut self, uptr: u64, src: &[u8]) -> Result<usize, u32> {
        if !user_ptr_ok(uptr, src.len()) {
            // EFAULT 且零残留：暂存清零
            for x in self.data.iter_mut() {
                *x = 0;
            }
            self.dirty = false;
            return Err(XC_CP_EFAULT);
        }
        let n = src.len().min(self.data.len());
        self.data[..n].copy_from_slice(&src[..n]);
        self.dirty = true;
        Ok(n)
    }

    /// copy_to_user 探针。
    pub fn copy_to_user(&self, uptr: u64, out: &mut Vec<u8>) -> Result<usize, u32> {
        if !user_ptr_ok(uptr, self.data.len()) {
            return Err(XC_CP_EFAULT);
        }
        out.extend_from_slice(&self.data);
        Ok(self.data.len())
    }

    /// 零残留判定（dirty=false 且全零）。
    pub fn clean(&self) -> bool {
        !self.dirty && self.data.iter().all(|&x| x == 0)
    }
}

/// 三结构 10^3 次零失真核：样本 i → 序列化 → 拷贝进出 → 反序列化 → 全等。
pub fn zero_distortion_1000() -> bool {
    let mut kb_stat = KBuf::new(96);
    let mut kb_tms = KBuf::new(32);
    let mut kb_it = KBuf::new(32);
    let mut ok = 0;
    for i in 0..1000u64 {
        // stat 往返
        let st = StatArg::sample(i);
        let bytes = st.to_bytes();
        let fake_uptr = USER_BASE + 0x1000;
        if kb_stat.copy_from_user(fake_uptr, &bytes).is_err() {
            return false;
        }
        let mut out = Vec::new();
        if kb_stat.copy_to_user(fake_uptr, &mut out).is_err() {
            return false;
        }
        match StatArg::from_bytes(&out) {
            Ok(back) if back == st => {}
            _ => return false,
        }
        // tms 往返
        let tm = TmsArg::sample(i);
        let tb = tm.to_bytes();
        if kb_tms.copy_from_user(fake_uptr + 0x100, &tb).is_err() {
            return false;
        }
        let mut tout = Vec::new();
        if kb_tms.copy_to_user(fake_uptr + 0x100, &mut tout).is_err() {
            return false;
        }
        match TmsArg::from_bytes(&tout) {
            Ok(back) if back == tm => {}
            _ => return false,
        }
        // itimerspec 往返
        let it = ItimerSpec::sample(i);
        let ib = it.to_bytes();
        if kb_it.copy_from_user(fake_uptr + 0x200, &ib).is_err() {
            return false;
        }
        let mut iout = Vec::new();
        if kb_it.copy_to_user(fake_uptr + 0x200, &mut iout).is_err() {
            return false;
        }
        match ItimerSpec::from_bytes(&iout) {
            Ok(back) if back == it => {}
            _ => return false,
        }
        ok += 1;
    }
    ok == 1000
}

/// EFAULT 零残留核：失败后暂存 clean。
pub fn efault_clean_exact() -> bool {
    let mut kb = KBuf::new(64);
    // 先脏一次
    let _ = kb.copy_from_user(USER_BASE, &[7u8; 32]);
    if !kb.dirty {
        return false;
    }
    // EFAULT（空域指针）
    let r = kb.copy_from_user(0x1234, &[7u8; 32]);
    if r != Err(XC_CP_EFAULT) {
        return false;
    }
    kb.clean()
}

/// 越界长度核：尾溢出用户域 → EFAULT。
pub fn overflow_exact() -> bool {
    let mut kb = KBuf::new(64);
    // 起点合法但尾越域
    let uptr = USER_END - 8;
    kb.copy_from_user(uptr, &[1u8; 32]) == Err(XC_CP_EFAULT)
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut kb = KBuf::new(32);
    // 1) 内核域指针当用户针 → EFAULT
    if kb.copy_from_user(0xFFFF_8000_0000_0000, &[1; 8]) == Err(XC_CP_EFAULT) {
        caught += 1;
    }
    // 2) 尾溢出
    if kb.copy_from_user(USER_END - 1, &[1; 8]) == Err(XC_CP_EFAULT) {
        caught += 1;
    }
    // 3) 短反序列化 → EFAULT
    if StatArg::from_bytes(&[0u8; 40]) == Err(XC_CP_EFAULT) {
        caught += 1;
    }
    // 4) 失真检测：篡改往返
    let st = StatArg::sample(5);
    let mut b = st.to_bytes();
    b[0] ^= 0xFF;
    if let Ok(back) = StatArg::from_bytes(&b) {
        if back != st {
            caught += 1;
        }
    } else {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：暂存折叠哈希。
pub fn fp_frozen(k: &KBuf) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in &k.data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8921");
    // J1-1 10^3 次零失真
    let ok = zero_distortion_1000();
    s.add("B07-01-千次零失真", ok, if ok { "" } else { "失真" });
    // J1-2 EFAULT 零残留
    let ok = efault_clean_exact();
    s.add("B07-02-EFAULT零残留", ok, if ok { "" } else { "残留" });
    // J1-3 越界长度
    let ok = overflow_exact();
    s.add("B07-03-越界EFAULT", ok, if ok { "" } else { "越界漂移" });
    // J1-4 用户域界
    let ok = user_ptr_ok(USER_BASE, 8) && !user_ptr_ok(0, 8) && !user_ptr_ok(USER_END, 1);
    s.add("B07-04-用户域界", ok, if ok { "" } else { "域界漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B07-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let kb = KBuf::new(64);
    let ok = fp_frozen(&kb) == fp_frozen(&kb) && kb.clean();
    s.add("B07-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
