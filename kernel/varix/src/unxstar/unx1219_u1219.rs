// unx1219_u1219 —— UNX-F1219 Polling 态文档册（B21 段域内文档）
// 判据原文 UNX-F1219-J1：B21 段文档覆盖全部判据条目（引用判据一致率 100%），
// 文档可导出（两次导出逐字节相同），指纹冻结。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb1f0 独占（0xb1f0~0xb22f）。
//
// 设计（纯功能 no_std）：
//   * 文档登记：条目（编号/标题/引用判据号集/摘要行数），B21 段 20 条全覆盖；
//   * 引用一致率：引用的判据号必须命中段内判据登记集（未知判据号拒）；
//   * 确定性导出：按编号稳定序拼接为字节序列，两次导出逐字节相同（幂等）；
//   * 覆盖守恒：段内判据集 ⊆ 被引用判据集（零缺角）；FNV 指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb1f0 独占） ─────────────────────────────────────────────
pub const XC_DC_SEALED: u32 = 0xB1F1; // 封账后登记
pub const XC_DC_DUP_NO: u32 = 0xB1F2; // 文档编号重复
pub const XC_DC_BAD_REF: u32 = 0xB1F3; // 引用未知判据号
pub const XC_DC_EMPTY_BODY: u32 = 0xB1F4; // 空文档（零摘要行）
pub const XC_DC_FULL: u32 = 0xB1F5; // 登记容量满
pub const XC_DC_COVER_GAP: u32 = 0xB1F6; // 段内判据缺覆盖

/// B21 段判据登记集（判据号闭集，F1201~F1220 逐条一线）。
pub const B21_JUDGE_IDS: [u32; 20] = [
    1201, 1202, 1203, 1204, 1205, 1206, 1207, 1208, 1209, 1210, 1211, 1212,
    1213, 1214, 1215, 1216, 1217, 1218, 1219, 1220,
];

/// 文档条目。
#[derive(Clone)]
pub struct DocEntry {
    /// 文档编号（段内唯一）。
    pub no: u32,
    /// 标题。
    pub title: &'static str,
    /// 引用判据号集（每条必须是 B21 段登记判据）。
    pub refs: Vec<u32>,
    /// 摘要行（正文）。
    pub body: Vec<&'static str>,
}

/// 文档册。
pub struct DocBook {
    cap: usize,
    entries: Vec<DocEntry>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DocBook {
    pub fn new(cap: usize) -> DocBook {
        DocBook { cap, entries: Vec::new(), sealed: false, rejects: Vec::new() }
    }

    fn known_judge(&self, jid: u32) -> bool {
        B21_JUDGE_IDS.iter().any(|&x| x == jid)
    }

    /// 登记文档条目。
    pub fn register(&mut self, e: DocEntry) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_DC_SEALED, "封账后登记"));
            return Err(XC_DC_SEALED);
        }
        if self.entries.len() >= self.cap {
            self.rejects.push((XC_DC_FULL, "登记容量满"));
            return Err(XC_DC_FULL);
        }
        for x in &self.entries {
            if x.no == e.no {
                self.rejects.push((XC_DC_DUP_NO, "文档编号重复"));
                return Err(XC_DC_DUP_NO);
            }
        }
        if e.body.is_empty() {
            self.rejects.push((XC_DC_EMPTY_BODY, "空文档"));
            return Err(XC_DC_EMPTY_BODY);
        }
        for r in &e.refs {
            if !self.known_judge(*r) {
                self.rejects.push((XC_DC_BAD_REF, "引用未知判据号"));
                return Err(XC_DC_BAD_REF);
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 覆盖守恒：B21 段判据集每条至少被一个文档引用（零缺角）。
    pub fn cover_complete(&self) -> bool {
        for j in B21_JUDGE_IDS.iter() {
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

    /// 引用一致率：全部引用命中登记集（登记侧已拒未知，此处对账复核）。
    pub fn ref_consistency(&self) -> (usize, usize) {
        let mut ok = 0usize;
        let mut total = 0usize;
        for e in &self.entries {
            for r in &e.refs {
                total += 1;
                if self.known_judge(*r) {
                    ok += 1;
                }
            }
        }
        (ok, total)
    }

    /// 确定性导出：按编号升序稳定拼接；两次调用逐字节相同。
    pub fn export(&self) -> Vec<u8> {
        // 稳定序：按 no 升序（选择排序，n 小无妨）
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
            // 行格式：DOC <no> <title> | J<ref>,J<ref> | body...
            out.extend_from_slice(b"DOC ");
            out.extend_from_slice(&itoa_bytes(e.no));
            out.push(b' ');
            out.extend_from_slice(e.title.as_bytes());
            out.push(b' ');
            out.push(b'|');
            for r in &e.refs {
                out.push(b'J');
                out.extend_from_slice(&itoa_bytes(*r));
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
            return Err(XC_DC_SEALED);
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

/// 简易 u32 → ASCII（无 std 依赖）。
fn itoa_bytes(mut v: u32) -> Vec<u8> {
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

/// 标准文档场景：20 条判据逐条建册（每条一文档），导出幂等对拍。
pub fn docbook_scenario() -> Result<(usize, u64, u64), u32> {
    let mut b = DocBook::new(32);
    for (i, jid) in B21_JUDGE_IDS.iter().enumerate() {
        b.register(DocEntry {
            no: 100 + i as u32,
            title: DOC_TITLES[i],
            refs: vec![*jid],
            body: vec![DOC_BODIES[i], "验收口径以判据原文为准。"],
        })?;
    }
    let (ok, total) = b.ref_consistency();
    if ok != total || total != 20 {
        return Err(XC_DC_BAD_REF);
    }
    if !b.cover_complete() {
        return Err(XC_DC_COVER_GAP);
    }
    let e1 = b.export();
    let e2 = b.export();
    if e1 != e2 {
        return Err(XC_DC_EMPTY_BODY);
    }
    let fp = b.seal()?;
    Ok((b.entries_len(), e1.len() as u64, fp))
}

/// B21 段文档标题（20 条，按判据号序）。
pub const DOC_TITLES: [&str; 20] = [
    "Polling 态执行主循环本体",
    "Polling 态事件采集点登记账",
    "Polling 态事件语义对齐账",
    "Polling 态延迟与负载画像",
    "Polling 态与中断事件对账",
    "Polling 态降级与回退路径",
    "Polling 态资源占用账",
    "Polling 态多源调度公平账",
    "Polling 态异常事件分类账",
    "Polling 态配置参数冻结",
    "Polling 态遥测上报口径",
    "Polling 态与 C 表联动面",
    "Polling 态回归资产与断言集",
    "Polling 态审计账（飞行记录）",
    "Polling 态统计矩阵",
    "Polling 态边界条件账",
    "Polling 态联动验证补充",
    "Polling 态启动序列挂接",
    "Polling 态文档册本体",
    "Polling 态 ktest 断言集",
];

/// B21 段文档正文首行（与标题一一对应）。
pub const DOC_BODIES: [&str; 20] = [
    "轮询事件泵全语义：逐轮记账、公平轮转、退出条件三要素。",
    "采集点登记：源清单、采样率、来源态四元齐备方可入账。",
    "轮询事件与中断事件同构化：字段逐位对齐，语义一致率 100%。",
    "延迟与负载画像：分位段记账，负载贡献基点化 Σ=10000bps。",
    "轮询/中断对账：同事件零重复零丢失，双向计数守恒。",
    "降级回退路径：轮询失效时回退中断模式的显性迁移账。",
    "资源占用账：每源内存与时间预算，超预算即立案。",
    "多源调度公平账：轮转配额、饥饿防护、公平度量化。",
    "异常事件分类：畸形/迟到/重复三分类闭集，逐例登记。",
    "配置参数冻结：参数表指纹冻结，变更即破指纹立案。",
    "遥测上报口径：上报字段白名单，越名单即拒。",
    "C 表联动面：向量路由复用 B16 分发表，一致率 100%。",
    "回归断言集：B21 段 ≥40 断言聚合，skip 必带因。",
    "审计飞行记录：进入/退出四元全记录，驻留账守恒。",
    "统计矩阵：per 源事件率与负载贡献逐源登记。",
    "边界条件账：空事件/洪峰/源抖动三分类，逐例带建议。",
    "联动验证补充：路由复用一致性复核与未登记向量拒。",
    "启动序列挂接：六步启动序按前置依赖执行，缺步不放行。",
    "文档册本体：覆盖 B21 段全部判据，引用一致率 100%。",
    "ktest 断言集：Polling 面 ≥40 断言宿主档全过。",
];

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）编号重复必拒
    {
        let mut b = DocBook::new(8);
        let e = DocEntry { no: 1, title: "t", refs: vec![1201], body: vec!["a"] };
        let _ = b.register(e.clone());
        if b.register(e) == Err(XC_DC_DUP_NO) {
            caught += 1;
        }
    }
    // 2）引用未知判据必拒
    {
        let mut b = DocBook::new(8);
        let e = DocEntry { no: 1, title: "t", refs: vec![9999], body: vec!["a"] };
        if b.register(e) == Err(XC_DC_BAD_REF) {
            caught += 1;
        }
    }
    // 3）空文档必拒
    {
        let mut b = DocBook::new(8);
        let e = DocEntry { no: 1, title: "t", refs: vec![1201], body: vec![] };
        if b.register(e) == Err(XC_DC_EMPTY_BODY) {
            caught += 1;
        }
    }
    // 4）容量满必拒
    {
        let mut b = DocBook::new(1);
        let _ = b.register(DocEntry { no: 1, title: "t", refs: vec![1201], body: vec!["a"] });
        if b.register(DocEntry { no: 2, title: "t", refs: vec![1202], body: vec!["a"] })
            == Err(XC_DC_FULL)
        {
            caught += 1;
        }
    }
    // 5）封账后登记必拒
    {
        let mut b = DocBook::new(8);
        let _ = b.register(DocEntry { no: 1, title: "t", refs: vec![1201], body: vec!["a"] });
        let _ = b.seal();
        if b.register(DocEntry { no: 2, title: "t", refs: vec![1202], body: vec!["a"] })
            == Err(XC_DC_SEALED)
        {
            caught += 1;
        }
    }
    // 6）覆盖缺角必被检出（少引一条）
    {
        let mut b = DocBook::new(32);
        for i in 0..19u32 {
            let _ = b.register(DocEntry {
                no: 100 + i,
                title: "t",
                refs: vec![B21_JUDGE_IDS[i as usize]],
                body: vec!["a"],
            });
        }
        // 缺 1220 覆盖
        if !b.cover_complete() {
            caught += 1;
        }
    }
    // 7）导出幂等（两次逐字节相同）
    {
        let mut b = DocBook::new(8);
        let _ = b.register(DocEntry { no: 5, title: "B", refs: vec![1202], body: vec!["x"] });
        let _ = b.register(DocEntry { no: 3, title: "A", refs: vec![1201], body: vec!["y"] });
        let e1 = b.export();
        let e2 = b.export();
        if e1 == e2 && e1.starts_with(b"DOC 3 A") {
            caught += 1;
        }
    }
    (caught, N)
}
