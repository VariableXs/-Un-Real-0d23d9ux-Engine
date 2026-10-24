//! veb300_mliclose —— VE-F0300 Mali 组收口移交。
//! 锚点（CoRun Unxreal 主册）：VE-F0300 · Mali 组收口移交 · 目标 340 行
//! 判据原文：VE-F0300-J1 组收口账：Mali 组十卡收口清单（卡
//!           号 281~290，各带落点模块名）；收口闸（十卡全
//!           收 + 认证阵证在持 → 组闭）；移交包（包序 + 卡
//!           清单 + 阵证指纹 + 目标 = 实验组）；移交校核
//!           （目标对 + 指纹非零 + 清单齐）；包重移交拒；
//!           实验组签收（入库 + 回执号 = seq × 0x2EF1 + 7）；
//!           回执单向核；收口总结（收口数 + 移交数 + 挂起
//!           数三数出账）。
//! 组件：收口清单 + 组闭闸 + 移交包 + 实验组签收。
//! 职责定位：Mali 组收口是「ARM 章的落印」——十卡全收才闭
//!           组：缺一卡的闭组是漏气的组；移交包是章与章之
//!           间的交接棒：阵证指纹随包走；回执单向可验是交
//!           棒后的确认：包号推回执，回执验不出假。
//! 性能逐项分解：收口 O(1)；校验 O(卡数)。
//! 诊断码段：0x31400（0x31401~0x31409 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MC_VERSION: &str = "veb300_mliclose-v1";

/// 诊断段基址（0x31400 段 · 本模块独占）。
pub const MC_SEG_BASE: u32 = 0x31400;
pub const XC_MAGIC: u32 = 0x31401; // 账魔数破
pub const XC_NOT_ALL: u32 = 0x31402; // 未全收
pub const XC_NO_CERT: u32 = 0x31403; // 无阵证
pub const XC_BAD_PK: u32 = 0x31404; // 包不合法
pub const XC_RESENT: u32 = 0x31405; // 包重移交
pub const XC_NOT_PK: u32 = 0x31406; // 签收无此包

/// 账魔数。
const MAGIC: u32 = 0x3140_1CDE;

/// Mali 组卡数。
pub const ML_CARDS: usize = 10;

/// 回执常数。
pub const REPLY_K: u64 = 0x2EF1;

/// 回执偏移。
pub const REPLY_B: u64 = 7;

/// 目标组：实验组（模型码）。
pub const TARGET_CODE: u32 = 0xEB07;

/// 移交包。
pub struct Pkg {
    pub seq: u64,
    /// 卡清单（281~290）。
    pub cards: [u32; ML_CARDS],
    pub fp: u64,
    pub target: u32,
}

/// Mali 收口账。
pub struct MaliClose {
    /// 十卡收口旗（索引 0~9 ↔ 卡 281~290）。
    pub done: [bool; ML_CARDS],
    /// 阵证指纹（零 = 无）。
    pub cert_fp: u64,
    handed: Vec<u64>,
    pub receipts: Vec<u64>,
    /// 挂起数（收口中挂起的卡）。
    pub suspended: u32,
    magic: u32,
}

impl MaliClose {
    /// 建。
    pub fn new() -> MaliClose {
        MaliClose {
            done: [false; ML_CARDS],
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

    /// 收口一卡（卡号 281~290）。
    pub fn close(&mut self, gpu: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if gpu < 281 || gpu > 290 {
            return Err(XC_MAGIC);
        }
        self.done[(gpu - 281) as usize] = true;
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
        let mut cards = [0u32; ML_CARDS];
        for i in 0..ML_CARDS {
            cards[i] = 281 + i as u32;
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
        for i in 0..ML_CARDS {
            if p.cards[i] != 281 + i as u32 {
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

    /// 实验组签收：回执 = seq × K + B。
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

    /// 收口总结三数。
    pub fn summary(&self) -> (usize, usize, u32) {
        let closed = {
            let mut n = 0;
            for f in self.done.iter() {
                if *f {
                    n += 1;
                }
            }
            n
        };
        (closed, self.handed_count(), self.suspended)
    }
}

pub fn run_veb300_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0300");
    let mut b = MaliClose::new();
    // 族零：卡号界。
    s.add("卡 280 拒", b.close(280) == Err(XC_MAGIC), "界外");
    s.add("卡 291 拒", b.close(291) == Err(XC_MAGIC), "界外");
    // 族一：收口闸。
    s.add("未收拒", b.gate() == Err(XC_NOT_ALL), "零卡");
    for gpu in 281..290u32 {
        let _ = b.close(gpu);
    }
    s.add("九卡仍拒", b.gate() == Err(XC_NOT_ALL), "差 290");
    s.add("收 290", b.close(290).is_ok(), "十卡齐");
    s.add("全收查", b.all_closed(), "清单满");
    s.add("无证拒", b.gate() == Err(XC_NO_CERT), "缺阵证");
    // 族二：入证闭组。
    b.put_cert(0xCAFE_00BA_B10C_0007);
    s.add("组闭", b.gate().is_ok(), "全收+证");
    // 族三：包校核。
    let p1 = match b.make_pkg(1) {
        Ok(p) => p,
        Err(_) => {
            s.add("造包败", false, "不应到");
            return s;
        }
    };
    s.add("目标实验组", p1.target == TARGET_CODE, "EB07");
    s.add("指纹随包", p1.fp == 0xCAFE_00BA_B10C_0007, "阵证");
    s.add("清单 281~290", p1.cards[0] == 281 && p1.cards[9] == 290, "十卡");
    s.add("包合法", b.check_pkg(&p1).is_ok(), "三核");
    // 篡改。
    let mut bad = Pkg { seq: 2, cards: p1.cards, fp: p1.fp, target: 0x1234 };
    s.add("目标篡改拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "非实验组");
    bad.target = TARGET_CODE;
    bad.fp = 0;
    s.add("零指纹拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "无证签");
    bad.fp = p1.fp;
    bad.cards[4] = 999;
    s.add("清单篡改拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "卡 285 变");
    // 族四：移交与重交。
    s.add("首交过", b.handover(&p1).is_ok(), "一交");
    s.add("重交拒", b.handover(&p1) == Err(XC_RESENT), "同包");
    s.add("移交 1", b.handed_count() == 1, "一包");
    // 族五：签收与回执。
    s.add("签收回执", b.sign_recv(&p1) == Ok(REPLY_K + REPLY_B), "seq1 回执");
    s.add("回执入账", b.receipts.len() == 1, "一执");
    // 未交签收拒。
    let p2 = match b.make_pkg(2) {
        Ok(p) => p,
        Err(_) => {
            s.add("造包 2 败", false, "不应到");
            return s;
        }
    };
    s.add("未交签收拒", b.sign_recv(&p2) == Err(XC_NOT_PK), "先交后签");
    // 回执核。
    s.add("真执可验", b.verify_reply(1, REPLY_K + REPLY_B), "单向");
    s.add("假执拒", b.verify_reply(1, REPLY_K + 8) == false, "偏移错");
    s.add("包错执拒", b.verify_reply(2, REPLY_K + REPLY_B) == false, "seq 错");
    // 族六：收口总结三数。
    let (closed, handed, susp) = b.summary();
    s.add("收口 10", closed == 10, "十卡");
    s.add("移交 1", handed == 1, "一包");
    s.add("挂起 0", susp == 0, "无挂");
    b.suspended = 2;
    s.add("挂起 2", b.summary().2 == 2, "三数出账");
    s.add("魔数核", MC_SEG_BASE == 0x31400, "段基址");
    s
}
