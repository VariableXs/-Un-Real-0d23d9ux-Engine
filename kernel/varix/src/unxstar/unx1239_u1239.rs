// unx1239_u1239 —— UNX-F1239 Draining 态文档册（B22 段域内文档）
// 判据原文 UNX-F1239-J1：B22 段文档覆盖全部判据条目（引用判据一致率 100%），
// 文档可导出（两次导出逐字节相同），指纹冻结。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb9f0 独占（0xb9f0~0xba2f）。
//
// 设计（纯功能 no_std）：
//   * 文档登记：条目（编号/标题/引用判据号/正文），B22 段 20 条全覆盖；
//   * 引用一致率：引用判据号必须命中段内判据集（未知拒）；
//   * 确定性导出：编号稳定序拼接，两次导出逐字节相同；
//   * 覆盖守恒：段内判据集 ⊆ 被引用集（零缺角）；FNV 指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb9f0 独占） ─────────────────────────────────────────────
pub const XC_DK_SEALED: u32 = 0xB9F1; // 封账后登记
pub const XC_DK_DUP_NO: u32 = 0xB9F2; // 编号重复
pub const XC_DK_BAD_REF: u32 = 0xB9F3; // 引用未知判据号
pub const XC_DK_EMPTY_BODY: u32 = 0xB9F4; // 空文档
pub const XC_DK_FULL: u32 = 0xB9F5; // 容量满
pub const XC_DK_COVER_GAP: u32 = 0xB9F6; // 覆盖缺角

/// B22 段判据集（F1221~F1240）。
pub const B22_JUDGE_IDS: [u32; 20] = [
    1221, 1222, 1223, 1224, 1225, 1226, 1227, 1228, 1229, 1230,
    1231, 1232, 1233, 1234, 1235, 1236, 1237, 1238, 1239, 1240,
];

/// 文档条目。
#[derive(Clone)]
pub struct DkEntry {
    pub no: u32,
    pub title: &'static str,
    pub refs: Vec<u32>,
    pub body: Vec<&'static str>,
}

/// 文档册。
pub struct DkBook {
    cap: usize,
    entries: Vec<DkEntry>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DkBook {
    pub fn new(cap: usize) -> DkBook {
        DkBook { cap, entries: vec![], sealed: false, rejects: vec![] }
    }

    fn known(&self, jid: u32) -> bool {
        B22_JUDGE_IDS.iter().any(|&x| x == jid)
    }

    pub fn register(&mut self, e: DkEntry) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_DK_SEALED, "封账后登记"));
            return Err(XC_DK_SEALED);
        }
        if self.entries.len() >= self.cap {
            self.rejects.push((XC_DK_FULL, "容量满"));
            return Err(XC_DK_FULL);
        }
        if e.body.is_empty() {
            self.rejects.push((XC_DK_EMPTY_BODY, "空文档"));
            return Err(XC_DK_EMPTY_BODY);
        }
        if self.entries.iter().any(|x| x.no == e.no) {
            self.rejects.push((XC_DK_DUP_NO, "编号重复"));
            return Err(XC_DK_DUP_NO);
        }
        for r in &e.refs {
            if !self.known(*r) {
                self.rejects.push((XC_DK_BAD_REF, "引用未知判据号"));
                return Err(XC_DK_BAD_REF);
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 覆盖守恒：段内判据每条至少被引用一次。
    pub fn cover_complete(&self) -> bool {
        for j in B22_JUDGE_IDS.iter() {
            let mut hit = false;
            for e in &self.entries {
                if e.refs.iter().any(|r| r == j) {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return false;
            }
        }
        true
    }

    /// 引用一致率。
    pub fn ref_consistency(&self) -> (usize, usize) {
        let mut ok = 0usize;
        let mut total = 0usize;
        for e in &self.entries {
            for r in &e.refs {
                total += 1;
                if self.known(*r) {
                    ok += 1;
                }
            }
        }
        (ok, total)
    }

    /// 确定性导出（编号升序稳定序）。
    pub fn export(&self) -> Vec<u8> {
        let mut order: Vec<usize> = vec![];
        for i in 0..self.entries.len() {
            order.push(i);
        }
        for i in 0..order.len() {
            let mut min = i;
            for j in i + 1..order.len() {
                if self.entries[order[j]].no < self.entries[order[min]].no {
                    min = j;
                }
            }
            let t = order[i];
            order[i] = order[min];
            order[min] = t;
        }
        let mut out: Vec<u8> = vec![];
        for idx in order {
            let e = &self.entries[idx];
            out.extend_from_slice(b"DOC ");
            out.extend_from_slice(&dk_itoa(e.no));
            out.push(b' ');
            out.extend_from_slice(e.title.as_bytes());
            out.push(b' ');
            out.push(b'|');
            for r in &e.refs {
                out.push(b'J');
                out.extend_from_slice(&dk_itoa(*r));
                out.push(b',');
            }
            out.push(b'|');
            for b in &e.body {
                out.push(b'\n');
                out.extend_from_slice(b.as_bytes());
            }
            out.push(b'\n');
        }
        out
    }

    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_DK_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for e in &self.entries {
            h ^= e.no as u64;
            h = h.wrapping_mul(0x0100_0193);
            for r in &e.refs {
                h ^= *r as u64;
                h = h.wrapping_mul(0x0100_0193);
            }
            for b in &e.body {
                for &byte in b.as_bytes() {
                    h ^= byte as u64;
                    h = h.wrapping_mul(0x0100_0193);
                }
            }
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

fn dk_itoa(mut v: u32) -> Vec<u8> {
    if v == 0 {
        return vec![b'0'];
    }
    let mut buf: Vec<u8> = vec![];
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    buf.reverse();
    buf
}

/// B22 段文档标题（20 条）。
pub const DK_TITLES: [&str; 20] = [
    "Draining 态执行本体",
    "Draining 态排空判据",
    "Draining 态双源对账强化",
    "Draining 态切换窗口账",
    "Draining 态超时保护账",
    "Draining 态事件幂等账",
    "Draining 态压测资产",
    "Draining 态故障注入",
    "Draining 态延迟账",
    "Draining 态对账底座联动",
    "Draining 态健康账",
    "Draining 态配置总表",
    "Draining 态人话化",
    "Draining 态统计矩阵",
    "Draining 态审计账",
    "Draining 态边界条件账",
    "Draining 态回归断言集",
    "Draining 态启动序列挂接",
    "Draining 态文档册本体",
    "Draining 态 ktest 断言集",
];

/// B22 段文档正文首行。
pub const DK_BODIES: [&str; 20] = [
    "双模并行排空主循环：逐轮消费、窗口记账、退出条件三要素。",
    "在途排空完成判定语义：零在途+窗口闭合双条件。",
    "同事件零重复零丢失：双源去重幂等、并集/交集/守恒三账。",
    "drain 窗口语义：开窗/关窗配对、漂移逐窗量化、超预算立案。",
    "三态降级机：完成/部分超时/放弃移交，余量显性结转。",
    "事件指纹去重：FNV 双哈希预筛+精确表确认，只执行一次。",
    "双源高压并行耐受：注入=消费守恒、两档压力预算。",
    "排空中断/半排空故障注入：逐例必抓、检出率 100%。",
    "排空窗口时长分布：8 桶直方图、P50/P95 上界、超阈立案。",
    "F0969 对账底座联动：窗号连续、预算带、Σ守恒、双侧对拍。",
    "F0899 口径注册：带内判定、三级分级、健康总分 bps。",
    "B22 参数收口：范围校验、冻结纪律、加法对偶允许。",
    "explain 报告：白名单制、确定性生成、隐私红线只报计数。",
    "双源事件率对照：差值千分比、失衡立案、均值/最值账。",
    "窗口与切换飞行记录：四元记录、驻留双算、回放对拍。",
    "空排空/风暴中切换：逐例证据与建议、分类守恒。",
    "B22 段 ≥30 断言聚合：三态执行、skip 必带因。",
    "五步排空启动序：前置依赖、缺步不放行。",
    "文档册本体：覆盖 B22 段全部判据、引用一致率 100%。",
    "ktest B22 断言集：Draining 面全量断言宿主档全过。",
];

/// 标准场景：20 判据逐条建册 + 导出幂等。
pub fn docbook_scenario() -> Result<(usize, u64, u64), u32> {
    let mut b = DkBook::new(32);
    for (i, jid) in B22_JUDGE_IDS.iter().enumerate() {
        b.register(DkEntry {
            no: 200 + i as u32,
            title: DK_TITLES[i],
            refs: vec![*jid],
            body: vec![DK_BODIES[i], "验收口径以判据原文为准。"],
        })?;
    }
    let (ok, total) = b.ref_consistency();
    if ok != total || total != 20 {
        return Err(XC_DK_BAD_REF);
    }
    if !b.cover_complete() {
        return Err(XC_DK_COVER_GAP);
    }
    let e1 = b.export();
    let e2 = b.export();
    if e1 != e2 {
        return Err(XC_DK_EMPTY_BODY);
    }
    let fp = b.seal()?;
    Ok((b.entries_len(), e1.len() as u64, fp))
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）编号重复必拒
    {
        let mut b = DkBook::new(8);
        let e = DkEntry { no: 1, title: "t", refs: vec![1221], body: vec!["a"] };
        let _ = b.register(e.clone());
        if b.register(e) == Err(XC_DK_DUP_NO) {
            caught += 1;
        }
    }
    // 2）引用未知判据必拒
    {
        let mut b = DkBook::new(8);
        if b.register(DkEntry { no: 1, title: "t", refs: vec![8888], body: vec!["a"] })
            == Err(XC_DK_BAD_REF)
        {
            caught += 1;
        }
    }
    // 3）空文档必拒
    {
        let mut b = DkBook::new(8);
        if b.register(DkEntry { no: 1, title: "t", refs: vec![1221], body: vec![] })
            == Err(XC_DK_EMPTY_BODY)
        {
            caught += 1;
        }
    }
    // 4）容量满必拒
    {
        let mut b = DkBook::new(1);
        let _ = b.register(DkEntry { no: 1, title: "t", refs: vec![1221], body: vec!["a"] });
        if b.register(DkEntry { no: 2, title: "t", refs: vec![1222], body: vec!["a"] })
            == Err(XC_DK_FULL)
        {
            caught += 1;
        }
    }
    // 5）封账后登记必拒
    {
        let mut b = DkBook::new(8);
        let _ = b.register(DkEntry { no: 1, title: "t", refs: vec![1221], body: vec!["a"] });
        let _ = b.seal();
        if b.register(DkEntry { no: 2, title: "t", refs: vec![1222], body: vec!["a"] })
            == Err(XC_DK_SEALED)
        {
            caught += 1;
        }
    }
    // 6）覆盖缺角必检出
    {
        let mut b = DkBook::new(32);
        for i in 0..19u32 {
            let _ = b.register(DkEntry {
                no: 100 + i,
                title: "t",
                refs: vec![B22_JUDGE_IDS[i as usize]],
                body: vec!["a"],
            });
        }
        if !b.cover_complete() {
            caught += 1;
        }
    }
    // 7）导出幂等 + 稳定序
    {
        let mut b = DkBook::new(8);
        let _ = b.register(DkEntry { no: 9, title: "B", refs: vec![1222], body: vec!["x"] });
        let _ = b.register(DkEntry { no: 2, title: "A", refs: vec![1221], body: vec!["y"] });
        let e1 = b.export();
        let e2 = b.export();
        if e1 == e2 && e1.starts_with(b"DOC 2 A") {
            caught += 1;
        }
    }
    (caught, N)
}
