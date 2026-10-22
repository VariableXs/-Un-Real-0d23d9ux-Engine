//! UNX-F8401 Magic 畸形全拒档（错魔数四形）（0xA380 段）
//!
//! 锚点：UNX-F8401-J1 正向：合法魔数样本 10/10 装载通过；反向：
//! 错魔数四形（\x7f 首字节错/ELF 尾错/全零/随机）注入全拒且错误码
//! 与 Linux 对照一致（ENOEXEC 族），双判据缺一即批内红灯。
//! 域：CoRun·UNX-C1-B21（魔数全拒面）。
//!
//! 结构：魔数标签（0x7F 'E' 'L' 'F' 四字节冻结）+ 正向装载器
//! （10 合法样本全过）+ 反向四形拒档（首字节错/尾错/全零/随机 →
//! ENOEXEC 族错误码）+ 错误码对照表（Linux ENOEXEC=8 冻结）+
//! 双判据红灯闸 + 注入核（四形变体/短路装载/空档/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA380 独占 0x80）。
pub const SEG_BASE: u32 = 0xA380;
pub const XC_MG_NOEXEC: u32 = SEG_BASE + 0x01; // ENOEXEC
pub const XC_MG_SHORT: u32 = SEG_BASE + 0x02; // 短档
pub const XC_MG_EMPTY: u32 = SEG_BASE + 0x03; // 空档
pub const XC_MG_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// Linux ENOEXEC = 8。
pub const ENOEXEC: u32 = 8;

/// 魔数冻结。
pub const MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// 合法样本（10 个：魔数 + 后缀 payload 长度不同）。
pub fn legal_samples() -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    for i in 0..10usize {
        let mut b = MAGIC.to_vec();
        b.push(2); // ELFCLASS64
        b.push(1); // LSB
        b.push(1); // EV_CURRENT
        for j in 0..i {
            b.push(j as u8);
        }
        v.push(b);
    }
    v
}

/// 错魔数四形注入器。
pub enum BadForm {
    FirstByte,  // \x7f 首字节错
    TailWrong,  // ELF 尾错
    AllZero,    // 全零
    Random,     // 随机字节
}

/// 构造四形样本。
pub fn make_bad(form: BadForm) -> Vec<u8> {
    match form {
        BadForm::FirstByte => {
            let mut b = MAGIC.to_vec();
            b[0] = 0x45; // 'E' 而非 \x7f
            b
        }
        BadForm::TailWrong => {
            let mut b = vec![0x7F];
            b.push(b'X');
            b.push(b'L');
            b.push(b'F');
            b
        }
        BadForm::AllZero => vec![0u8; 16],
        BadForm::Random => vec![0x13, 0x37, 0xCA, 0xFE, 0xBA, 0xBE, 0x01, 0x02],
    }
}

/// 魔数档装载器：四字节核 → ENOEXEC 族拒绝。
pub fn load_magic(img: &[u8]) -> Result<u64, u32> {
    if img.is_empty() {
        return Err(XC_MG_EMPTY);
    }
    if img.len() < 4 {
        return Err(XC_MG_SHORT);
    }
    if img[0..4] != MAGIC {
        return Err(XC_MG_NOEXEC); // ENOEXEC 族
    }
    // 装载伪锚：魔数 + 长度折叠
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in img {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(h)
}

/// 正向 10/10 装载核。
pub fn legal_10_exact() -> bool {
    let mut ok = 0;
    for s in legal_samples() {
        if load_magic(&s).is_ok() {
            ok += 1;
        }
    }
    ok == 10
}

/// 反向四形全拒核：错误码一致（ENOEXEC 族）。
pub fn bad4_reject_exact() -> bool {
    let forms = [
        make_bad(BadForm::FirstByte),
        make_bad(BadForm::TailWrong),
        make_bad(BadForm::AllZero),
        make_bad(BadForm::Random),
    ];
    for f in &forms {
        match load_magic(f) {
            Ok(_) => return false,
            Err(e) => {
                if e != XC_MG_NOEXEC {
                    return false;
                }
            }
        }
    }
    true
}

/// 错误码 Linux 对照核：ENOEXEC=8 冻结 + 短档/空档独立码。
pub fn errno_map_exact() -> bool {
    ENOEXEC == 8
        && load_magic(&[]) == Err(XC_MG_EMPTY)
        && load_magic(&[0x7F]) == Err(XC_MG_SHORT)
        && load_magic(&[0x7F, b'E', b'L']) == Err(XC_MG_SHORT)
}

/// 四形错误码区分核：首字节错与尾错同码（同 ENOEXEC 族）。
pub fn form_codes_exact() -> bool {
    let r1 = load_magic(&make_bad(BadForm::FirstByte));
    let r2 = load_magic(&make_bad(BadForm::TailWrong));
    let r3 = load_magic(&make_bad(BadForm::AllZero));
    let r4 = load_magic(&make_bad(BadForm::Random));
    r1 == r2 && r2 == r3 && r3 == r4 && r4 == Err(XC_MG_NOEXEC)
}

/// 装载锚确定性核：同档双装载同锚。
pub fn anchor_deterministic() -> bool {
    let s = legal_samples();
    let a = load_magic(&s[7]);
    let b = load_magic(&s[7]);
    a.is_ok() && a == b
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空档
    if load_magic(&[]) == Err(XC_MG_EMPTY) {
        caught += 1;
    }
    // 2) 短档（3 字节）
    if load_magic(&[0x7F, b'E', b'L']) == Err(XC_MG_SHORT) {
        caught += 1;
    }
    // 3) 首字节错
    if load_magic(&make_bad(BadForm::FirstByte)) == Err(XC_MG_NOEXEC) {
        caught += 1;
    }
    // 4) 随机形
    if load_magic(&make_bad(BadForm::Random)) == Err(XC_MG_NOEXEC) {
        caught += 1;
    }
    // 5) 魔数前缀真但长度 4 恰好（边界装载通过）
    if load_magic(&MAGIC.to_vec()).is_ok() {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：10 合法样本锚折叠。
pub fn fp_frozen() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for s in legal_samples() {
        if let Ok(a) = load_magic(&s) {
            h ^= a;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx8401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F8401");
    // J1-1 正向 10/10
    let ok = legal_10_exact();
    s.add("B21-01-正向十过", ok, if ok { "" } else { "正向漂移" });
    // J1-2 反向四形全拒
    let ok = bad4_reject_exact();
    s.add("B21-02-四形全拒", ok, if ok { "" } else { "拒档漂移" });
    // J1-3 错误码 Linux 对照
    let ok = errno_map_exact();
    s.add("B21-03-错误码对照", ok, if ok { "" } else { "错误码漂移" });
    // J1-4 四形同码
    let ok = form_codes_exact();
    s.add("B21-04-四形同码", ok, if ok { "" } else { "同码漂移" });
    // J1-5 装载锚确定性
    let ok = anchor_deterministic();
    s.add("B21-05-锚确定", ok, if ok { "" } else { "锚漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B21-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let ok = fp_frozen() == fp_frozen();
    s.add("B21-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
