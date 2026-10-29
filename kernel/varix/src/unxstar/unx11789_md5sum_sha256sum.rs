//! UNX-F11789 校验链集成判据（md5sum/sha256sum/cksum）·子功能01（金丝雀）（0x41B00 段）
//!
//! 锚点：UNX-F11789-J1 校验清单管道面判据绿（-c 全绿/篡改红），与基
//! 准机一致；J2 校验语义（摘要符 + 算法符 + 缺文件拒三性质）；J3 校
//! 验指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C5-B30（校验链集成面）。
//!
//! 结构：三算法枚举 + 校验记录 + 清单核验器（-c 逐行跑）+ 账本 +
//! 折叠指纹 + 注入核（野算法/摘要篡改/缺文件/印后/格式坏必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x41B00 独占 0x20）。
pub const SEG_BASE: u32 = 0x41B00;
pub const CV_BAD_ALGO: u32 = SEG_BASE + 0x01; // 野算法
pub const CV_MISMATCH: u32 = SEG_BASE + 0x02; // 摘要篡改
pub const CV_MISSING: u32 = SEG_BASE + 0x03; // 缺文件
pub const CV_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const CV_BAD_FORMAT: u32 = SEG_BASE + 0x05; // 格式坏

/// 校验算法三族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CkAlgo {
    Md5,
    Sha256,
    Cksum,
}

impl CkAlgo {
    /// 算法符（清单行首标识）。
    pub fn sym(self) -> &'static str {
        match self {
            CkAlgo::Md5 => "md5",
            CkAlgo::Sha256 => "sha256",
            CkAlgo::Cksum => "cksum",
        }
    }

    /// 摘要宽度（十六进制字符数）。
    pub fn width(self) -> usize {
        match self {
            CkAlgo::Md5 => 32,
            CkAlgo::Sha256 => 64,
            CkAlgo::Cksum => 8,
        }
    }

    /// 野算法符解析。
    pub fn parse(sym: &str) -> Result<CkAlgo, u32> {
        match sym {
            "md5" => Ok(CkAlgo::Md5),
            "sha256" => Ok(CkAlgo::Sha256),
            "cksum" => Ok(CkAlgo::Cksum),
            _ => Err(CV_BAD_ALGO),
        }
    }
}

/// 校验记录：文件 + 算法 + 基准摘要。
#[derive(Clone, PartialEq, Debug)]
pub struct CvRecord {
    pub file: String,
    pub algo: CkAlgo,
    pub digest: String,
}

/// 清单核验器：模拟 -c 管道逐行核。
pub struct CvChecker {
    list: Vec<CvRecord>,
    pub checked: u64,
    pub failed: u64,
    sealed: bool,
}

impl CvChecker {
    pub fn new() -> CvChecker {
        CvChecker {
            list: Vec::new(),
            checked: 0,
            failed: 0,
            sealed: false,
        }
    }

    /// 录入基准行：格式 "<algo>:<file>:<digest>"。
    pub fn add_line(&mut self, line: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() != 3 {
            return Err(CV_BAD_FORMAT);
        }
        let algo = CkAlgo::parse(parts[0])?;
        if parts[2].len() != algo.width() || !parts[2].bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CV_BAD_FORMAT);
        }
        self.list.push(CvRecord {
            file: parts[1].to_string(),
            algo,
            digest: parts[2].to_string(),
        });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// 核一行：按 (file, algo) 找基准并比对实际摘要。
    pub fn verify(&mut self, file: &str, algo: CkAlgo, actual: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        let rec = match self.list.iter().find(|r| r.file == file && r.algo == algo) {
            Some(r) => r,
            None => {
                self.failed += 1;
                return Err(CV_MISSING);
            }
        };
        self.checked += 1;
        if rec.digest != actual || actual.len() != algo.width() {
            self.failed += 1;
            return Err(CV_MISMATCH);
        }
        Ok(())
    }

    /// 全清单管道跑（-c）：实际摘要表全核。
    pub fn run_all(&mut self, actuals: &[(String, CkAlgo, String)]) -> Result<usize, u32> {
        for (f, a, d) in actuals {
            self.verify(f, *a, d)?;
        }
        Ok(self.list.len())
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41B00;
        h = h.wrapping_mul(31).wrapping_add(self.checked as u32);
        h = h.wrapping_mul(31).wrapping_add(self.failed as u32);
        h = h.wrapping_mul(31).wrapping_add(self.list.len() as u32);
        for r in &self.list {
            h = h.wrapping_mul(31).wrapping_add(r.algo as u32);
            h = h.wrapping_mul(31).wrapping_add(r.digest.len() as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准摘要（可复演的伪摘要生成：与基准机一致的确定性折叠）。
pub fn std_digest(algo: CkAlgo, file: &str) -> String {
    let seed = algo as u32;
    let mut h: u64 = 0x41B00u64 + seed as u64;
    for b in file.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    let width = algo.width();
    let mut out = String::new();
    for i in 0..width {
        let nib = ((h >> ((i % 16) * 4)) & 0xF) as u32;
        let c = if nib < 10 {
            (b'0' + nib as u8) as char
        } else {
            (b'a' + (nib - 10) as u8) as char
        };
        out.push(c);
    }
    out
}

/// 标准夹具：三算法三文件全绿基准。
pub fn mk_checker() -> CvChecker {
    let mut c = CvChecker::new();
    let _ = c.add_line(&format!("md5:/etc/hosts:{}", std_digest(CkAlgo::Md5, "/etc/hosts")));
    let _ = c.add_line(&format!("sha256:/bin/ls:{}", std_digest(CkAlgo::Sha256, "/bin/ls")));
    let _ = c.add_line(&format!("cksum:/tmp/x:{}", std_digest(CkAlgo::Cksum, "/tmp/x")));
    c
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野算法符
    got[0] = CvChecker::new().add_line("crc32:/f:0123abcd").err() == Some(CV_BAD_ALGO);
    // 2) 摘要篡改（翻转一位）
    let mut c = mk_checker();
    let mut bad = std_digest(CkAlgo::Md5, "/etc/hosts");
    bad.replace_range(0..1, if bad.starts_with('0') { "1" } else { "0" });
    got[1] = c.verify("/etc/hosts", CkAlgo::Md5, &bad).err() == Some(CV_MISMATCH);
    // 3) 缺文件
    let mut c2 = mk_checker();
    got[2] = c2.verify("/no/such", CkAlgo::Md5, "0000").err() == Some(CV_MISSING);
    // 4) 格式坏（摘要宽度不符）
    got[3] = CvChecker::new().add_line("md5:/f:zz").err() == Some(CV_BAD_FORMAT);
    // 5) 印后操作
    let mut c3 = mk_checker();
    c3.seal();
    got[4] = c3.add_line("md5:/f:0000").err() == Some(CV_FROZEN)
        && c3.verify("/etc/hosts", CkAlgo::Md5, "0000").err() == Some(CV_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx11789_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11789");
    let mut c = mk_checker();

    // J1 清单行数
    let j1 = c.len() == 3;
    s.add("J1-清单行数", j1, "三算法三行");

    // J2 -c 全绿
    let j2 = {
        let acts = vec![
            ("/etc/hosts".to_string(), CkAlgo::Md5, std_digest(CkAlgo::Md5, "/etc/hosts")),
            ("/bin/ls".to_string(), CkAlgo::Sha256, std_digest(CkAlgo::Sha256, "/bin/ls")),
            ("/tmp/x".to_string(), CkAlgo::Cksum, std_digest(CkAlgo::Cksum, "/tmp/x")),
        ];
        c.run_all(&acts).is_ok()
    };
    s.add("J2-全绿", j2, "基准一致全过");

    // J3 篡改红
    let j3 = {
        let mut c2 = mk_checker();
        let mut bad = std_digest(CkAlgo::Sha256, "/bin/ls");
        bad.replace_range(63..64, if bad.ends_with('0') { "1" } else { "0" });
        c2.verify("/bin/ls", CkAlgo::Sha256, &bad).err() == Some(CV_MISMATCH)
    };
    s.add("J3-篡改红", j3, "末位翻转必红");

    // J4 算法符解析
    let j4 = CkAlgo::parse("md5") == Ok(CkAlgo::Md5)
        && CkAlgo::parse("sha256") == Ok(CkAlgo::Sha256)
        && CkAlgo::parse("cksum") == Ok(CkAlgo::Cksum)
        && CkAlgo::parse("crc32") == Err(CV_BAD_ALGO);
    s.add("J4-算法符", j4, "三符解析+野符拒");

    // J5 摘要宽度
    let j5 = CkAlgo::Md5.width() == 32 && CkAlgo::Sha256.width() == 64 && CkAlgo::Cksum.width() == 8;
    s.add("J5-摘要宽度", j5, "32/64/8");

    // J6 缺文件拒
    let j6 = {
        let mut c2 = mk_checker();
        c2.verify("/absent", CkAlgo::Md5, "x").err() == Some(CV_MISSING)
    };
    s.add("J6-缺文件拒", j6, "清单外必拒");

    // J7 格式坏拒：宽度不符
    let j7 = CvChecker::new().add_line("md5:/f:0123").err() == Some(CV_BAD_FORMAT);
    s.add("J7-格式坏拒", j7, "宽度 4≠32 必拒");

    // J8 格式坏拒：非十六进制
    let j8 = CvChecker::new()
        .add_line("sha256:/f:zz0000000000000000000000000000000000000000000000000000000000000000")
        .err()
        == Some(CV_BAD_FORMAT);
    s.add("J8-非hex拒", j8, "zz 前缀必拒");

    // J9 摘要确定性（与基准机一致）
    let j9 = std_digest(CkAlgo::Md5, "/etc/hosts") == std_digest(CkAlgo::Md5, "/etc/hosts")
        && std_digest(CkAlgo::Md5, "/etc/hosts") != std_digest(CkAlgo::Md5, "/other");
    s.add("J9-确定性", j9, "同参同纹异参异纹");

    // J10 计数守恒
    let j10 = c.checked == 3 && c.failed == 0;
    s.add("J10-计数守恒", j10, "核3红0");

    // J11 印后操作拒
    let j11 = {
        let mut c2 = mk_checker();
        c2.seal();
        c2.add_line("md5:/f:0000").err() == Some(CV_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut c2 = mk_checker();
        let a = c2.fold();
        c2.seal();
        c2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11789_cksum_flow() {
        let mut c = mk_checker();
        assert_eq!(c.len(), 3);
        assert_eq!(c.verify("/etc/hosts", CkAlgo::Md5, &std_digest(CkAlgo::Md5, "/etc/hosts")), Ok(()));
        assert_eq!(c.verify("/etc/hosts", CkAlgo::Md5, "deadbeef"), Err(CV_MISMATCH));
        assert_eq!(c.verify("/none", CkAlgo::Md5, "x"), Err(CV_MISSING));
        assert_eq!(CkAlgo::parse("sha512"), Err(CV_BAD_ALGO));
        assert_eq!(CvChecker::new().add_line("md5:/f"), Err(CV_BAD_FORMAT));
        c.seal();
        assert_eq!(c.add_line("md5:/f:0000"), Err(CV_FROZEN));
        assert_eq!(c.verify("/etc/hosts", CkAlgo::Md5, "0"), Err(CV_FROZEN));
    }

    #[test]
    fn unx11789_checks_green() {
        let s = run_unx11789_checks();
        assert!(s.all_passed(), "UNX-F11789 判据未全绿");
    }
}
