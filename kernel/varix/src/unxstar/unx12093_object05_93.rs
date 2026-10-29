//! UNX-F12093 OBJECT_TYPE_INITIALIZER 类型对象语义预告（0x19dc0 段）
//!
//! 卡面：OBJECT_TYPE_INITIALIZER——类型对象初始化语义：池标签 ASCII
//! 大写四元组、默认对象尺寸上限、回调位族闭集、删除回调须配关闭回调、
//! 类型名唯一。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 池标签合法：四字节皆 ASCII 大写字母且非零必检。
//! - J2 尺寸上限：默认对象尺寸非零且 ≤ 0x1000。
//! - J3 回调位族闭集：未知回调位非零必拒。
//! - J4 删除配对：置 DELETE 回调而未置 CLOSE 回调必拒。
//! - J5 类型名唯一：同名类型二次登记必拒（含空名）。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：OtFlags 回调位族 + OtInit 类型初始化档 + OtBook 类型册 +
//! 折叠指纹（211 进制卷积）+ 注入核（坏标签/坏尺寸/越位族/缺配对/印后登）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19dc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19dc0;
pub const OT_BAD_TAG: u32 = SEG_BASE + 0x01; // 池标签非法
pub const OT_BAD_SIZE: u32 = SEG_BASE + 0x02; // 尺寸越限
pub const OT_BAD_FLAG: u32 = SEG_BASE + 0x03; // 越位族
pub const OT_FLAG_PAIR: u32 = SEG_BASE + 0x04; // 缺配对
pub const OT_DUP: u32 = SEG_BASE + 0x05; // 重名（含空名）
pub const OT_FROZEN: u32 = SEG_BASE + 0x06; // 印账后登记

/// 尺寸上限。
pub const MAX_DEFAULT_SIZE: u32 = 0x1000;

/// 回调位族。
pub const CB_CLOSE: u32 = 0x0000_0001;
pub const CB_DELETE: u32 = 0x0000_0002;
pub const CB_PARSE: u32 = 0x0000_0004;
pub const CB_OK_TO_CLOSE: u32 = 0x0000_0008;
pub const CB_KNOWN: u32 = CB_CLOSE | CB_DELETE | CB_PARSE | CB_OK_TO_CLOSE;

/// 类型初始化档。
#[derive(Clone)]
pub struct OtInit {
    pub name: String,
    pub pool_tag: u32,
    pub default_size: u32,
    pub flags: u32,
}

impl OtInit {
    /// 池标签四字节 ASCII 大写判定。
    pub fn tag_ok(&self) -> bool {
        let bs = self.pool_tag.to_be_bytes();
        bs.iter().all(|b| (b'A'..=b'Z').contains(b))
    }
    /// 校验核。
    pub fn validate(&self) -> Result<(), u32> {
        if !self.tag_ok() {
            return Err(OT_BAD_TAG);
        }
        if self.default_size == 0 || self.default_size > MAX_DEFAULT_SIZE {
            return Err(OT_BAD_SIZE);
        }
        if self.flags & !CB_KNOWN != 0 {
            return Err(OT_BAD_FLAG);
        }
        if self.flags & CB_DELETE != 0 && self.flags & CB_CLOSE == 0 {
            return Err(OT_FLAG_PAIR);
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!(
            "T{}|tag{:x}|sz{}|cb{:x}",
            self.name, self.pool_tag, self.default_size, self.flags
        )
    }
}

/// 类型册。
pub struct OtBook {
    pub recs: Vec<OtInit>,
    pub sealed: bool,
}

impl OtBook {
    pub fn new() -> OtBook {
        OtBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：名唯一（含空名）→ 校验核。
    pub fn put(&mut self, t: OtInit) -> Result<(), u32> {
        if self.sealed {
            return Err(OT_FROZEN);
        }
        if t.name.is_empty() || self.recs.iter().any(|r| r.name == t.name) {
            return Err(OT_DUP);
        }
        t.validate()?;
        self.recs.push(t);
        Ok(())
    }

    pub fn by_name(&self, name: &str) -> Option<&OtInit> {
        self.recs.iter().find(|r| r.name == name)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.pool_tag);
            h = h.wrapping_mul(211).wrapping_add(r.default_size);
            h = h.wrapping_mul(211).wrapping_add(r.flags);
            for by in r.name.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("types={}", self.recs.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, name: &str) -> String {
        match self.by_name(name) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = OtBook::new();
    // 1) 坏池标签（含小写）必拒。
    got[0] = b.put(OtInit {
        name: "T1".to_string(), pool_tag: 0x6162_6364, // "abcd" 小写
        default_size: 64, flags: 0,
    }).err() == Some(OT_BAD_TAG);
    // 2) 坏尺寸（超上限）必拒。
    got[1] = b.put(OtInit {
        name: "T1".to_string(), pool_tag: 0x5052_4f43, // "PROC"
        default_size: 0x2000, flags: 0,
    }).err() == Some(OT_BAD_SIZE);
    // 3) 越位族必拒。
    got[2] = b.put(OtInit {
        name: "T1".to_string(), pool_tag: 0x5052_4f43,
        default_size: 64, flags: 0x8000_0000,
    }).err() == Some(OT_BAD_FLAG);
    // 4) 缺配对必拒（DELETE 无 CLOSE）。
    got[3] = b.put(OtInit {
        name: "T1".to_string(), pool_tag: 0x5052_4f43,
        default_size: 64, flags: CB_DELETE,
    }).err() == Some(OT_FLAG_PAIR);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(OtInit {
        name: "T1".to_string(), pool_tag: 0x5052_4f43,
        default_size: 64, flags: CB_CLOSE | CB_DELETE,
    }).err() == Some(OT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12093_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12093");
    let mk = |flags: u32| OtInit {
        name: "Test".to_string(),
        pool_tag: 0x5445_5354, // "TEST"
        default_size: 64,
        flags,
    };

    // J1 池标签：大写过、小写拒、零拒。
    let j1 = {
        let ok = mk(0);
        let low = OtInit { pool_tag: 0x7465_7374, ..mk(0) }; // "test"
        let zero = OtInit { pool_tag: 0, ..mk(0) };
        ok.tag_ok() && !low.tag_ok() && !zero.tag_ok()
    };
    s.add("J1-池标签", j1, "大写四元组闭检");

    // J2 尺寸上限：零与超限拒，边界 0x1000 过。
    let j2 = {
        let zero = OtInit { default_size: 0, ..mk(0) };
        let over = OtInit { default_size: 0x1001, ..mk(0) };
        let edge = OtInit { default_size: MAX_DEFAULT_SIZE, ..mk(0) };
        zero.validate().err() == Some(OT_BAD_SIZE)
            && over.validate().err() == Some(OT_BAD_SIZE)
            && edge.validate().is_ok()
    };
    s.add("J2-尺寸上限", j2, "零拒超限拒边界过");

    // J3 回调位族闭集。
    let j3 = {
        let t = OtInit { flags: CB_PARSE | 0x100, ..mk(0) };
        t.validate().err() == Some(OT_BAD_FLAG)
    };
    s.add("J3-位族闭集", j3, "族外位必拒");

    // J4 删除配对。
    let j4 = {
        let bad = mk(CB_DELETE);
        let good = mk(CB_DELETE | CB_CLOSE);
        bad.validate().err() == Some(OT_FLAG_PAIR) && good.validate().is_ok()
    };
    s.add("J4-删除配对", j4, "DELETE 须配 CLOSE");

    // J5 纯 CLOSE 无配对要求。
    let j5 = mk(CB_CLOSE).validate().is_ok();
    s.add("J5-纯 CLOSE 放行", j5, "单向回调无配对");

    // J6 名唯一与空名拒。
    let j6 = {
        let mut b = OtBook::new();
        let _ = b.put(mk(0));
        b.put(mk(0)).err() == Some(OT_DUP)
            && b.put(OtInit { name: String::new(), ..mk(0) }).err() == Some(OT_DUP)
    };
    s.add("J6-名唯一", j6, "重名空名皆拒");

    // J7 渲染行。
    let j7 = {
        let mut b = OtBook::new();
        let _ = b.put(mk(CB_CLOSE));
        b.line_of("Test").contains("54455354") && b.line_of("Nope") == "missing"
    };
    s.add("J7-渲染行", j7, "标签十六进制回读");

    // J8 名查询。
    let j8 = {
        let mut b = OtBook::new();
        let _ = b.put(mk(0));
        b.by_name("Test").is_some() && b.by_name("x").is_none()
    };
    s.add("J8-名查询", j8, "双通道查询");

    // J9 印账冻结。
    let j9 = {
        let mut b = OtBook::new();
        b.seal();
        b.put(mk(0)).err() == Some(OT_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = OtBook::new();
        let _ = b.put(mk(0));
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = OtBook::new();
    let _ = b.put(OtInit {
        name: "Process".to_string(), pool_tag: 0x5052_4f43,
        default_size: 0x300, flags: CB_CLOSE | CB_DELETE,
    });
    let _ = b.put(OtInit {
        name: "File".to_string(), pool_tag: 0x4649_4c45,
        default_size: 0x200, flags: CB_CLOSE,
    });

    s.add("摘要回读", b.summary().contains("types=2"), "双类型在册");
    s.add("渲染回读", b.line_of("Process").contains("sz768"), "尺寸 0x300 回读");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12093_validate_flow() {
        let good = OtInit {
            name: "A".to_string(), pool_tag: 0x4142_4344,
            default_size: 0x1000, flags: CB_OK_TO_CLOSE,
        };
        assert!(good.validate().is_ok());
        let bad = OtInit { flags: CB_DELETE, ..good };
        assert_eq!(bad.validate(), Err(OT_FLAG_PAIR));
    }

    #[test]
    fn unx12093_checks_green() {
        let s = run_unx12093_checks();
        assert!(s.all_passed(), "UNX-F12093 判据未全绿");
    }

    #[test]
    fn book_dup_guard() {
        let mut b = OtBook::new();
        let t = OtInit {
            name: "X".to_string(), pool_tag: 0x4153_4d45,
            default_size: 16, flags: 0,
        };
        assert!(b.put(t.clone()).is_ok());
        assert_eq!(b.put(t), Err(OT_DUP));
    }
}
