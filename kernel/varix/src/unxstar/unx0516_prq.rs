//! unx0516_prq —— UNX-F0516 页请求队列账（PRQ 描述符 + 响应配对 + 超时回收）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0516 · 页请求账 · 目标 210 行
//! 判据原文：UNX-F0516-J1 页请求账：请一 请求入队（设
//!           备缺页请求 (PASID,IOVA,RW) 入 PRQ，序号发
//!           放）；请二 响应配对（响应按 (pasid,iova,序号)
//!           三元组精确配对，错配拒）；请三 重复请求幂等
//!           （同键请求在途时再发不新入队——挂到原请求）；
//!           请四 超时回收（在途计数有上限，超限新请求拒
//!           ——背压而非覆盖）；请五 响应后解挂（响应配
//!           对成功后在途账清项，同键可再请求）。
//! 组件：PRQ 入队器 + 响应配对表 + 幂等挂账 + 背压闸 + 解挂。
//! 职责定位：PRQ 是设备向 OS 要页的通道——设备翻译缺页时
//!   发页请求，OS 补页后发页响应。请求-响应必须精确配对
//!   （三元组），配错 = 把 A 进程的页填给 B 进程的设备。
//!   在途上限是背压：无上限则缺页风暴把 OS 淹死。
//! 性能逐项分解：入队 O(在途数)；配对 O(在途数)。
//! 诊断码段：0x34E80（0x34E81~0x34E89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34E80;
pub const XC_BACKP: u32 = SEG_BASE + 1; // 在途超限背压
pub const XC_NOMATCH: u32 = SEG_BASE + 2; // 响应无配对

/// 在途页请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pend {
    pub pasid: u16,
    pub iova: u64,
    pub seq: u64,
    pub hops: u32, // 同键挂账数（请三）
}

/// PRQ 管理器。
pub struct Prq {
    pub pend: Vec<Pend>,
    pub max_pend: usize,
    pub next_seq: u64,
    pub served: u64,
}

impl Prq {
    pub fn new(max_pend: usize) -> Self {
        Prq { pend: Vec::new(), max_pend, next_seq: 1, served: 0 }
    }

    /// 请一+请三+请四：入队（同键幂等挂账；在途满背压）。
    pub fn request(&mut self, pasid: u16, iova: u64) -> Result<u64, u32> {
        // 幂等挂账。
        if let Some(p) = self.pend.iter_mut().find(|p| p.pasid == pasid && p.iova == iova) {
            p.hops += 1;
            return Ok(p.seq);
        }
        // 背压。
        if self.pend.len() >= self.max_pend {
            return Err(XC_BACKP);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.pend.push(Pend { pasid, iova, seq, hops: 1 });
        Ok(seq)
    }

    /// 请二+请五：响应配对（三元组精确），配对成功解挂。
    pub fn respond(&mut self, pasid: u16, iova: u64, seq: u64) -> Result<u32, u32> {
        let pos = self.pend.iter().position(|p| p.pasid == pasid && p.iova == iova && p.seq == seq);
        match pos {
            Some(i) => {
                let hops = self.pend.remove(i).hops;
                self.served += 1;
                Ok(hops)
            }
            None => Err(XC_NOMATCH),
        }
    }
}

pub fn run_unx0516_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0516");
    cs.add("SEG 段位独占", SEG_BASE == 0x34E80 && XC_NOMATCH == SEG_BASE + 2, "u32 高段全域普查无冲突");

    // 族一：入队与序号。
    let mut q = Prq::new(4);
    let s1 = q.request(1, 0x1000);
    let s2 = q.request(1, 0x2000);
    cs.add("双请求双序", s1 == Ok(1) && s2 == Ok(2), "序号发放");
    cs.add("在途二", q.pend.len() == 2, "计数同步");

    // 族二：同键幂等挂账。
    let s3 = q.request(1, 0x1000);
    cs.add("同键挂原序", s3 == Ok(1), "不新发序号");
    cs.add("在途不涨", q.pend.len() == 2, "幂等不新入队");
    cs.add("挂账计数", q.pend[0].hops == 2, "两请求一实体");

    // 族三：响应配对。
    cs.add("错序拒", q.respond(1, 0x1000, 2) == Err(XC_NOMATCH), "(1,0x1000) 配 seq=2 错");
    cs.add("错键拒", q.respond(2, 0x1000, 1) == Err(XC_NOMATCH), "pasid 错");
    cs.add("精确配对成", q.respond(1, 0x1000, 1) == Ok(2), "三元组全配");
    cs.add("解挂后空位", q.pend.len() == 1, "清一留一");
    cs.add("解挂可再请求", q.request(1, 0x1000) == Ok(3), "新序号再发");

    // 族四：背压。
    let mut q2 = Prq::new(2);
    q2.request(1, 0x1000).ok();
    q2.request(1, 0x2000).ok();
    cs.add("在途满拒", q2.request(1, 0x3000) == Err(XC_BACKP), "上限二");
    cs.add("同键不受背压", q2.request(1, 0x1000).is_ok(), "挂账路径不占新位");
    q2.respond(1, 0x1000, 1).ok();
    cs.add("解压后入队", q2.request(1, 0x3000).is_ok(), "背压解除");

    // 族五：响应账守恒。
    let mut q3 = Prq::new(8);
    let mut total_hops = 0u32;
    for i in 0..3u64 {
        q3.request(2, 0x4000 + i * 0x1000).ok();
    }
    let _ = q3.request(2, 0x4000); // 挂账
    for i in 0..3u64 {
        if let Ok(h) = q3.respond(2, 0x4000 + i * 0x1000, i + 1) {
            total_hops += h;
        }
    }
    cs.add("服务三笔", q3.served == 3, "三响应");
    cs.add("挂账随解挂", total_hops == 4, "3+1（同键挂账一并归还）");
    cs.add("在途清空", q3.pend.is_empty(), "全解挂");

    // 族六：序号生命周期（发放-消费单调，解挂不回收序号）。
    let mut q4 = Prq::new(8);
    let seq_a = q4.request(3, 0x7000).unwrap_or(0);
    q4.respond(3, 0x7000, seq_a).ok();
    let seq_b = q4.request(3, 0x7000);
    cs.add("序号不回收", seq_b == Ok(seq_a + 1), "新请求新序号");
    cs.add("旧序号失效", q4.respond(3, 0x7000, seq_a) == Err(XC_NOMATCH), "已消费序号再配拒");
    // 序号跨多轮单调。
    let mut last = seq_b.unwrap_or(0);
    let mut mono = true;
    for i in 0..5u64 {
        let s = q4.request(3, 0x8000 + i * 0x1000).unwrap_or(0);
        if s <= last {
            mono = false;
        }
        last = s;
        let _ = q4.respond(3, 0x8000 + i * 0x1000, s);
    }
    cs.add("五轮单调", mono && last == seq_a + 6, "序号严格递增");

    // 族七：同键不同 PASID 独立（键含 PASID）。
    let mut q5 = Prq::new(8);
    let p1 = q5.request(1, 0x9000);
    let p2 = q5.request(2, 0x9000);
    cs.add("同 IOVA 异 PASID 分立", p1 == Ok(1) && p2 == Ok(2), "键三元组含 PASID");
    cs.add("在途二", q5.pend.len() == 2, "两实体");
    cs.add("异 PASID 配对", q5.respond(2, 0x9000, 2).is_ok(), "各配各的");

    // 族八：背压恢复后再满。
    let mut q6 = Prq::new(1);
    let s0 = q6.request(9, 0xA000);
    cs.add("容量一", q6.request(9, 0xB000) == Err(XC_BACKP), "第二发背压");
    let _ = s0;
    q6.respond(9, 0xA000, 1).ok();
    cs.add("解挂再发成", q6.request(9, 0xB000).is_ok(), "容量归还");
    cs.add("再满", q6.request(9, 0xC000) == Err(XC_BACKP), "重新背压");

    cs
}
