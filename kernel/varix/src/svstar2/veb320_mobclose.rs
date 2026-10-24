//! veb320_mobclose —— VE-F0320 移动组收口移交。
//! 锚点（CoRun Unxreal 主册）：VE-F0320 · 移动组收口移交 · 目标 340 行
//! 判据原文：VE-F0320-J1 收口移交账：移动组二十卡收口清单
//!           （卡号 301~320）；收口闸（二十卡全收 + 认证/阵
//!           证在持 → 组闭）；移交包（包序 + 卡清单 + 阵证
//!           指纹 + 目标 = 桌面组）；包校核（目标 + 指纹 +
//!           清单三核）；重移交拒；桌面组签收（回执 = seq ×
//!           0x2EF3 + 11）；回执单向核；总结三数（收口数 /
//!           移交数 / 挂起数）。
//! 组件：收口清单 + 组闭闸 + 移交包 + 桌面组签收。
//! 职责定位：移动组收口是「移动章的落印」——二十卡全收才闭
//!           组：二十张卡一张都不能少；移交包是移动章与桌面
//!           章的交接棒：阵证指纹随包走；回执单向可验是交棒
//!           后的确认：桌面组拿包推回执，移动组拿包号核回执。
//! 性能逐项分解：收口 O(1)；校验 O(卡数)。
//! 诊断码段：0x31900（0x31901~0x31909 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MX_VERSION: &str = "veb320_mobclose-v1";

/// 诊断段基址（0x31900 段 · 本模块独占）。
pub const MX_SEG_BASE: u32 = 0x31900;
pub const XC_MAGIC: u32 = 0x31901; // 账魔数破
pub const XC_NOT_ALL: u32 = 0x31902; // 未全收
pub const XC_NO_CERT: u32 = 0x31903; // 无阵证
pub const XC_BAD_PK: u32 = 0x31904; // 包不合法
pub const XC_RESENT: u32 = 0x31905; // 重移交
pub const XC_NOT_PK: u32 = 0x31906; // 签收无此包

/// 账魔数。
const MAGIC: u32 = 0x3190_1CE0;

/// 移动组卡数。
pub const MB_CARDS: usize = 20;

/// 回执常数。
pub const REPLY_K: u64 = 0x2EF3;
/// 回执偏移。
pub const REPLY_B: u64 = 11;

/// 目标组：桌面组。
pub const TARGET_CODE: u32 = 0xDE56;

/// 移交包。
pub struct Pkg {
    pub seq: u64,
    /// 卡清单（301~320）。
    pub cards: [u32; MB_CARDS],
    pub fp: u64,
    pub target: u32,
}

/// 收口账。
pub struct MobClose {
    pub done: [bool; MB_CARDS],
    pub cert_fp: u64,
    handed: Vec<u64>,
    pub receipts: Vec<u64>,
    pub suspended: u32,
    magic: u32,
}

impl MobClose {
    /// 建。
    pub fn new() -> MobClose {
        MobClose {
            done: [false; MB_CARDS],
            cert_fp: 0,
            handed: Vec::new(),
            receipts: Vec::new(),
            suspended: 0,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 收口一卡（301~320）。
    pub fn close(&mut self, gpu: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if gpu < 301 || gpu > 320 {
            return Err(XC_MAGIC);
        }
        self.done[(gpu - 301) as usize] = true;
        Ok(())
    }

    /// 全收？
    pub fn all_closed(&self) -> bool {
        for f in self.done.iter() {
            if !f {
                return false;
            }
        }
        true
    }

    /// 入阵证。
    pub fn put_cert(&mut self, fp: u64) {
        self.cert_fp = fp;
    }

    /// 组闭闸。
    pub fn gate(&self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if !self.all_closed() {
            return Err(XC_NOT_ALL);
        }
        if self.cert_fp == 0 {
            return Err(XC_NO_CERT);
        }
        Ok(())
    }

    /// 造包。
    pub fn make_pkg(&self, seq: u64) -> Result<Pkg, u32> {
        self.gate()?;
        let mut cards = [0u32; MB_CARDS];
        for i in 0..MB_CARDS {
            cards[i] = 301 + i as u32;
        }
        Ok(Pkg { seq, cards, fp: self.cert_fp, target: TARGET_CODE })
    }

    /// 包校核。
    pub fn check_pkg(&self, p: &Pkg) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if p.target != TARGET_CODE {
            return Err(XC_BAD_PK);
        }
        if p.fp == 0 {
            return Err(XC_BAD_PK);
        }
        for i in 0..MB_CARDS {
            if p.cards[i] != 301 + i as u32 {
                return Err(XC_BAD_PK);
            }
        }
        Ok(())
    }

    /// 移交。
    pub fn handover(&mut self, p: &Pkg) -> Result<(), u32> {
        self.check_pkg(p)?;
        for h in self.handed.iter() {
            if *h == p.seq {
                return Err(XC_RESENT);
            }
        }
        self.handed.push(p.seq);
        Ok(())
    }

    /// 桌面组签收：回执 = seq × K + B。
    pub fn sign_recv(&mut self, p: &Pkg) -> Result<u64, u32> {
        self.check_pkg(p)?;
        let mut seen = false;
        for h in self.handed.iter() {
            if *h == p.seq {
                seen = true;
            }
        }
        if !seen {
            return Err(XC_NOT_PK);
        }
        let rc = p.seq * REPLY_K + REPLY_B;
        self.receipts.push(rc);
        Ok(rc)
    }

    /// 回执单向核。
    pub fn verify_reply(&self, seq: u64, rc: u64) -> bool {
        rc == seq * REPLY_K + REPLY_B
    }

    /// 移交数。
    pub fn handed_count(&self) -> usize {
        self.handed.len()
    }

    /// 总结三数。
    pub fn summary(&self) -> (usize, usize, u32) {
        let mut n = 0;
        for f in self.done.iter() {
            if *f {
                n += 1;
            }
        }
        (n, self.handed_count(), self.suspended)
    }
}

pub fn run_veb320_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0320");
    let mut b = MobClose::new();
    // 族零：卡号界。
    s.add("卡 300 拒", b.close(300) == Err(XC_MAGIC), "界外");
    s.add("卡 321 拒", b.close(321) == Err(XC_MAGIC), "界外");
    // 族一：收口闸。
    s.add("未收拒", b.gate() == Err(XC_NOT_ALL), "零卡");
    for gpu in 301..320u32 {
        let _ = b.close(gpu);
    }
    s.add("十九卡仍拒", b.gate() == Err(XC_NOT_ALL), "差 320");
    s.add("收 320", b.close(320).is_ok(), "二十卡齐");
    s.add("全收查", b.all_closed(), "清单满");
    s.add("无证拒", b.gate() == Err(XC_NO_CERT), "缺阵证");
    // 族二：入证闭组。
    b.put_cert(0xB0CA_7E00_FADE_0001);
    s.add("组闭", b.gate().is_ok(), "全收+证");
    // 族三：包校核。
    let p1 = match b.make_pkg(1) {
        Ok(p) => p,
        Err(_) => {
            s.add("造包败", false, "不应到");
            return s;
        }
    };
    s.add("目标桌面组", p1.target == TARGET_CODE, "DE56");
    s.add("指纹随包", p1.fp == 0xB0CA_7E00_FADE_0001, "阵证");
    s.add("清单 301~320", p1.cards[0] == 301 && p1.cards[19] == 320, "二十卡");
    s.add("包合法", b.check_pkg(&p1).is_ok(), "三核");
    // 篡改核。
    let mut bad = Pkg { seq: 2, cards: p1.cards, fp: p1.fp, target: 0x1234 };
    s.add("目标篡改拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "非桌面组");
    bad.target = TARGET_CODE;
    bad.fp = 0;
    s.add("零指纹拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "无证签");
    bad.fp = p1.fp;
    bad.cards[9] = 999;
    s.add("清单篡改拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "卡 310 变");
    // 族四：移交与重交。
    s.add("首交过", b.handover(&p1).is_ok(), "一交");
    s.add("重交拒", b.handover(&p1) == Err(XC_RESENT), "同包");
    s.add("移交 1", b.handed_count() == 1, "一包");
    // 族五：签收与回执。
    s.add("签收回执", b.sign_recv(&p1) == Ok(REPLY_K + REPLY_B), "seq1 回执");
    s.add("回执入账", b.receipts.len() == 1, "一执");
    let p2 = match b.make_pkg(2) {
        Ok(p) => p,
        Err(_) => {
            s.add("造包 2 败", false, "不应到");
            return s;
        }
    };
    s.add("未交签收拒", b.sign_recv(&p2) == Err(XC_NOT_PK), "先交后签");
    s.add("真执可验", b.verify_reply(1, REPLY_K + REPLY_B), "单向");
    s.add("假执拒", b.verify_reply(1, REPLY_K + 12) == false, "偏移错");
    s.add("包错执拒", b.verify_reply(2, REPLY_K + REPLY_B) == false, "seq 错");
    // 族六：总结三数。
    let (closed, handed, susp) = b.summary();
    s.add("收口 20", closed == 20, "二十卡");
    s.add("移交 1", handed == 1, "一包");
    s.add("挂起 0", susp == 0, "无挂");
    b.suspended = 3;
    s.add("挂起 3", b.summary().2 == 3, "三数出账");
    s.add("魔数核", MX_SEG_BASE == 0x31900, "段基址");
    s
}
