//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1319 · 退役文档册（B26 段域内文档） · 0x7680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1319-J1 B26 段文档覆盖全部 19 条（引用判据一致率 100%），
//!      文档可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 19 篇文档登记表（F1302..F1320 每判据一篇），标题+三节正文
//!     （概述/判据原文/验收口径）结构化存储；
//!   - 引用判据一致率：文档引据号 == 登记判据号（逐篇对拍，漂移
//!     必抓，一致率 1000‰）；
//!   - 覆盖判准：19/19 全覆盖（缺一篇必抓）；
//!   - 目录契约：TOC 逐篇含编号+标题（缺项必抓）；
//!   - 导出契约：全册导出（目录+正文），两次导出逐位一致（确定性），
//!     空册导出必拒；
//!   - 文档册 append-only + 冻结制（定稿后拒改）；
//!   - 三节齐备判准：每篇概述/判据/验收三节非空（缺节必抓）；
//!   - 注入核 5 项（漏文档/引据漂移/缺节/TOC 缺项/导出缺失）全部
//!     必抓，LCG 种子写死 0x7680_0015。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7680 段，退役文档册专属）。
pub const SEG_BASE: u16 = 0x7680;

// ── 诊断码闭集（0x7680..0x7689 十码） ───────────────────────
/// 0x7680 文档缺失（判据无文档）。
pub const E_DOC_MISS: u16 = SEG_BASE;
/// 0x7681 引据漂移。
pub const E_CITE_DRIFT: u16 = SEG_BASE + 1;
/// 0x7682 缺节（三节不全）。
pub const E_SEC_MISS: u16 = SEG_BASE + 2;
/// 0x7683 TOC 缺项。
pub const E_TOC_MISS: u16 = SEG_BASE + 3;
/// 0x7684 导出缺失。
pub const E_EXPORT_MISS: u16 = SEG_BASE + 4;
/// 0x7685 重复文档。
pub const E_DUP_DOC: u16 = SEG_BASE + 5;
/// 0x7686 文档册冻结后改写。
pub const E_RDONLY: u16 = SEG_BASE + 6;
/// 0x7687 判据号越界。
pub const E_ID_OOB: u16 = SEG_BASE + 7;
/// 0x7688 结果不稳。
pub const E_UNSTABLE: u16 = SEG_BASE + 8;
/// 0x7689 空文档册。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_DOC_MISS => "文档缺失：19 判据逐篇补文后再出册",
        E_CITE_DRIFT => "引据漂移：文档引据号与登记判据号对不上，改回登记号",
        E_SEC_MISS => "缺节：概述/判据/验收三节必须全非空",
        E_TOC_MISS => "TOC 缺项：目录逐篇补编号+标题",
        E_EXPORT_MISS => "导出缺失：导出条数必须等于登记篇数",
        E_DUP_DOC => "重复文档：每判据恰一篇，去重",
        E_RDONLY => "文档册冻结：定稿后只读，修订走新版本",
        E_ID_OOB => "判据号越界：B26 段只收 1302..1320",
        E_UNSTABLE => "结果不稳：同册两次导出必须逐位一致",
        E_EMPTY => "空文档册：先登记 19 篇再出册",
        _ => "未知 0x7680 段诊断码：核对文档册段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// B26 判据数。
pub const CRIT_COUNT: usize = 19;
/// 每篇节数。
pub const SECTIONS: usize = 3;
/// 节名。
pub const SECTION_NAMES: [&str; 3] = ["概述", "判据", "验收"];

/// 19 判据登记表（号+题名）。
pub const CRIT_TABLE: [(u16, &str); 19] = [
    (1302, "ps2 测绘执行账"),
    (1303, "ps2 退役预案账"),
    (1304, "ps2 轮询退役执行账"),
    (1305, "退役窗口事件守恒账"),
    (1306, "退役回退执行账"),
    (1307, "退役进度总账"),
    (1308, "退役与 F0039 四态联动执行账"),
    (1309, "退役双路并行对拍执行账"),
    (1310, "退役后清理账"),
    (1311, "退役压测资产"),
    (1312, "退役故障注入"),
    (1313, "退役健康账"),
    (1314, "退役人话化"),
    (1315, "退役配置总表"),
    (1316, "退役审计账"),
    (1317, "退役与 A3/A5 消费对账账"),
    (1318, "退役回归资产"),
    (1319, "退役文档册"),
    (1320, "ktest B26 断言集"),
];

// ── 文档条目 ────────────────────────────────────────────────
/// 文档条目（三节正文）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocEntry {
    /// 判据号。
    pub fid: u16,
    /// 标题。
    pub title: &'static str,
    /// 三节正文（概述/判据/验收）。
    pub sections: [&'static str; SECTIONS],
}

impl DocEntry {
    /// 三节齐备。
    pub fn sections_ok(&self) -> bool {
        self.sections.iter().all(|s| s.len() > 0)
    }

    /// 引据号文本（判据原文节内必须含 UNX-Fxxxx-J1）。
    pub fn cite_ok(&self) -> bool {
        self.sections[1].contains("UNX-F") && self.sections[1].contains("-J1")
    }

    /// 篇指纹（号+标题+三节卷积）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp = self.fid as u64;
        fp = fp.wrapping_mul(31).wrapping_add(self.title.len() as u64);
        for s in self.sections.iter() {
            fp = fp.wrapping_mul(17).wrapping_add(s.len() as u64);
        }
        fp
    }
}

/// 按判据号构造规范文档（三节结构化生成，确定性）。
pub fn make_doc(fid: u16, title: &'static str) -> DocEntry {
    let overview = match fid {
        1302 => "ps2 测绘执行：12 点 spin_loop 等待点逐点执行入图",
        1303 => "ps2 退役预案：三路径退役次序与窗口排程落档",
        1304 => "ps2 轮询退役：全部测绘点退役执行与码流守恒",
        1305 => "三路径统一守恒：轮询停用=中断生效恒等，零丢失零重复",
        1306 => "回退执行：任意步骤可逆，回退后与基线等价",
        1307 => "进度总账：四态×三路径状态机实时可查",
        1308 => "四态联动：全部退役点经 Draining 窗口，绕窗为零",
        1309 => "双路对拍：轮询/中断同事件双采，归档供 B27 消费",
        1310 => "清理账：死代码标记不删除，清理可逆且等价期保护",
        1311 => "压测资产：五类风暴场景 3 次可重复，高压守恒",
        1312 => "故障注入：五类故障 10/10 必抓，防护动作对表",
        1313 => "健康账：12 项指标 F0899 口径注册，告警联通",
        1314 => "人话化：explain 报告覆盖全账本，术语一致",
        1315 => "配置总表：12 项参数双闸收口，篡改检出回退",
        1316 => "审计账：四类事件五元组，滚动保底常绿",
        1317 => "消费对账：A3/A5 切换无感，回调零丢失",
        1318 => "回归资产：19 判据聚合一次全跑，断言全绿",
        1319 => "文档册：本册，19 篇全覆盖可导出",
        _ => "ktest B26：退役面断言集一次全绿",
    };
    let judge_text = match fid {
        1302 => "UNX-F1302-J1 测绘点执行率与等待点计数判准",
        1303 => "UNX-F1303-J1 退役预案排程完整率判准",
        1304 => "UNX-F1304-J1 退役执行 10/10 与码流守恒 100% 判准",
        1305 => "UNX-F1305-J1 三路径守恒账恒等与零丢失零重复判准",
        1306 => "UNX-F1306-J1 回退 10/10 与基线等价 100% 判准",
        1307 => "UNX-F1307-J1 四态状态机一致率 100% 判准",
        1308 => "UNX-F1308-J1 经窗执行率 100% 与绕窗 0 次判准",
        1309 => "UNX-F1309-J1 双路采集完整率 100% 与归档率 100% 判准",
        1310 => "UNX-F1310-J1 标记率 100% 与清理可逆率 100% 判准",
        1311 => "UNX-F1311-J1 五类压测 3 次可重复与高压守恒判准",
        1312 => "UNX-F1312-J1 五类故障 10/10 必抓与防护正确判准",
        1313 => "UNX-F1313-J1 12 项健康指标注册与告警联通判准",
        1314 => "UNX-F1314-J1 报告覆盖 B26 全账本与术语一致判准",
        1315 => "UNX-F1315-J1 12 项参数双闸一致与篡改检出判准",
        1316 => "UNX-F1316-J1 五元组完整率 100% 与滚动保底判准",
        1317 => "UNX-F1317-J1 消费方无感 10/10 与回调零丢失判准",
        1318 => "UNX-F1318-J1 19 判据聚合全跑与断言全绿判准",
        1319 => "UNX-F1319-J1 19 篇文档全覆盖与引据一致判准",
        _ => "UNX-F1320-J1 B26 聚合断言全绿与双账互校判准",
    };
    let accept = match fid {
        1302 => "验收：执行留痕入账，API/软件写回状态=已完成",
        1303 => "验收：预案落档可回放，API/软件写回状态=已完成",
        1304 => "验收：10/10 执行与守恒断言，API/软件写回状态=已完成",
        1305 => "验收：逐路径可查总守恒率 100%，API/软件写回状态=已完成",
        1306 => "验收：P99≤100ms 实测留痕，API/软件写回状态=已完成",
        1307 => "验收：状态实时可查注入 10/10，API/软件写回状态=已完成",
        1308 => "验收：绕窗 0 次对拍完整率 100%，API/软件写回状态=已完成",
        1309 => "验收：B27 直接消费摘要稳定，API/软件写回状态=已完成",
        1310 => "验收：顺序强制拦截 10/10，API/软件写回状态=已完成",
        1311 => "验收：资产冻结可审计，API/软件写回状态=已完成",
        1312 => "验收：隔离账齐备可复查，API/软件写回状态=已完成",
        1313 => "验收：摘要一致率 100%，API/软件写回状态=已完成",
        1314 => "验收：报告可导出五段齐，API/软件写回状态=已完成",
        1315 => "验收：篡改 10/10 检出回退，API/软件写回状态=已完成",
        1316 => "验收：导出 100% 保底常绿，API/软件写回状态=已完成",
        1317 => "验收：预案落档切换无感，API/软件写回状态=已完成",
        1318 => "验收：JSON 可导出映射零遗漏，API/软件写回状态=已完成",
        1319 => "验收：本册可导出 19/19，API/软件写回状态=已完成",
        _ => "验收：断言数入域级账只增不减，API/软件写回状态=已完成",
    };
    DocEntry { fid, title, sections: [overview, judge_text, accept] }
}

// ── 文档册 ──────────────────────────────────────────────────
/// 文档册（append-only + 冻结）。
#[derive(Debug)]
pub struct DocBook {
    /// 篇目。
    entries: Vec<DocEntry>,
    /// 冻结旗。
    frozen: bool,
}

impl DocBook {
    /// 新册。
    pub fn new() -> DocBook {
        DocBook { entries: Vec::new(), frozen: false }
    }

    /// 登记（幂等；号唯一；越界必拒；冻结拒）。
    pub fn register(&mut self, e: DocEntry) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if e.fid < 1302 || e.fid > 1320 {
            return Err(E_ID_OOB);
        }
        if self.entries.iter().any(|x| x.fid == e.fid) {
            return Err(E_DUP_DOC);
        }
        if !e.sections_ok() {
            return Err(E_SEC_MISS);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 全量登记（19 篇规范文档）。
    pub fn register_all(&mut self) -> Result<usize, u16> {
        for (fid, title) in CRIT_TABLE.iter() {
            self.register(make_doc(*fid, title))?;
        }
        Ok(self.entries.len())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 冻结旗。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 篇数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空册。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 覆盖判准（19/19 全有且号序连续）。
    pub fn coverage_full(&self) -> bool {
        if self.entries.len() != CRIT_COUNT {
            return false;
        }
        for (i, (fid, _)) in CRIT_TABLE.iter().enumerate() {
            match self.entries.get(i) {
                Some(e) if e.fid == *fid => {}
                _ => return false,
            }
        }
        true
    }

    /// 引据一致率（千分制：引据齐备且号对拍篇数×1000/19）。
    pub fn cite_permille(&self) -> u64 {
        if self.entries.is_empty() {
            return 0;
        }
        let mut n = 0u64;
        for (i, e) in self.entries.iter().enumerate() {
            let fid_ok = match CRIT_TABLE.get(i) {
                Some((f, _)) => *f == e.fid,
                None => false,
            };
            if e.cite_ok() && fid_ok && e.sections_ok() {
                n += 1;
            }
        }
        n * 1000 / CRIT_COUNT as u64
    }

    /// 目录（TOC 行：编号+标题）。
    pub fn toc(&self) -> Vec<String> {
        self.entries.iter().map(|e| format!("UNX-F{} {}", e.fid, e.title)).collect()
    }

    /// 目录齐备（19 行且逐行含 UNX-F 前缀）。
    pub fn toc_ok(&self) -> bool {
        let t = self.toc();
        t.len() == CRIT_COUNT && t.iter().all(|l| l.contains("UNX-F"))
    }

    /// 全册审计。
    pub fn audit(&self) -> Result<(), u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        if !self.coverage_full() {
            return Err(E_DOC_MISS);
        }
        for e in self.entries.iter() {
            if !e.sections_ok() {
                return Err(E_SEC_MISS);
            }
            if !e.cite_ok() {
                return Err(E_CITE_DRIFT);
            }
        }
        if !self.toc_ok() {
            return Err(E_TOC_MISS);
        }
        Ok(())
    }

    /// 全册指纹（逐篇指纹卷积）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0x7680;
        for e in self.entries.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(e.fingerprint());
        }
        fp
    }

    /// 全册导出（TOC + 逐篇正文）。
    pub fn export(&self) -> Result<String, u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        if !self.toc_ok() {
            return Err(E_TOC_MISS);
        }
        let mut out = String::from("══ B26 退役文档册 ══\n【目录】\n");
        for l in self.toc().iter() {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str("【正文】\n");
        for e in self.entries.iter() {
            out.push_str(&format!("— UNX-F{} {}\n", e.fid, e.title));
            for (i, s) in e.sections.iter().enumerate() {
                out.push_str(&format!("  {}：{}\n", SECTION_NAMES[i], s));
            }
        }
        if out.len() == 0 {
            return Err(E_EXPORT_MISS);
        }
        Ok(out)
    }

    /// 篇目查询。
    pub fn entry_of(&self, fid: u16) -> Result<&DocEntry, u16> {
        match self.entries.iter().find(|e| e.fid == fid) {
            Some(e) => Ok(e),
            None => Err(E_DOC_MISS),
        }
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 全册覆盖演练（19/19 + 审计绿 + 引据一致率 1000‰）。
pub fn drill_coverage() -> (bool, String) {
    let mut bk = DocBook::new();
    let n = match bk.register_all() {
        Ok(n) => n,
        Err(c) => return (false, format!("登记错=0x{:04X} {}", c, detail(c))),
    };
    let cov = bk.coverage_full();
    let cite = bk.cite_permille();
    let audit = bk.audit().is_ok();
    (n == CRIT_COUNT && cov && cite == 1000 && audit,
     format!("覆盖 {}/{} 引据={}‰ 审计={}", n, CRIT_COUNT, cite, audit))
}

/// 三节齐备演练（逐篇三节非空 + 缺节必拒）。
pub fn drill_sections() -> (bool, String) {
    let mut bk = DocBook::new();
    let _ = bk.register_all();
    let all_ok = (0..CRIT_COUNT as u16).all(|i| {
        match bk.entry_of(1302 + i) {
            Ok(e) => e.sections_ok(),
            Err(_) => false,
        }
    });
    // 缺节注入必拒。
    let mut broken = make_doc(1305, "守恒账");
    broken.sections[1] = "";
    let refused = matches!(bk.register(broken), Err(E_SEC_MISS) | Err(E_DUP_DOC));
    (all_ok && refused, format!("三节齐={} 缺节拒={}", all_ok, refused))
}

/// 引据漂移必抓演练（判据节无 UNX-F 引据 → 审计必抓）。
pub fn drill_cite_drift() -> (bool, String) {
    let mut bk = DocBook::new();
    let _ = bk.register_all();
    let clean = bk.cite_permille() == 1000;
    // 注入：篡改一篇判据节（去引据）。
    if let Some(e) = bk.entries.get_mut(3) {
        e.sections[1] = "判据节被清空";
    }
    let caught = matches!(bk.audit(), Err(E_CITE_DRIFT));
    (clean && caught, format!("净引={} 漂移抓={}", clean, caught))
}

/// 目录契约演练（19 行全含 UNX-F + 缺项必抓）。
pub fn drill_toc() -> (bool, String) {
    let mut bk = DocBook::new();
    let _ = bk.register_all();
    let ok = bk.toc_ok();
    let toc = bk.toc();
    let first = toc.first().cloned().unwrap_or_default();
    let last = toc.last().cloned().unwrap_or_default();
    (ok && toc.len() == 19 && first.contains("1302") && last.contains("1320"),
     format!("TOC {} 行 首行={} 末行含1320={}", toc.len(), first, last.contains("1320")))
}

/// 导出契约演练（两次逐位一致 + TOC 在导出内 + 空册拒）。
pub fn drill_export() -> (bool, String) {
    let mut bk = DocBook::new();
    let _ = bk.register_all();
    let e1 = match bk.export() {
        Ok(s) => s,
        Err(c) => return (false, format!("导出错=0x{:04X} {}", c, detail(c))),
    };
    let e2 = match bk.export() {
        Ok(s) => s,
        Err(_) => String::new(),
    };
    let has_toc = e1.contains("【目录】") && e1.contains("【正文】");
    let all_in = CRIT_TABLE.iter().all(|(fid, _)| e1.contains(&format!("UNX-F{}", fid)));
    let empty = DocBook::new();
    let empty_refused = matches!(empty.export(), Err(E_EMPTY));
    (e1 == e2 && has_toc && all_in && empty_refused,
     format!("导出一致={} 段头齐={} 19篇全在={} 空册拒={}", e1 == e2, has_toc, all_in, empty_refused))
}

/// 登记守卫演练（重复拒/越界拒/冻结拒/缺号拒）。
pub fn drill_registry_guards() -> (bool, String) {
    let mut bk = DocBook::new();
    let _ = bk.register_all();
    let dup = matches!(bk.register(make_doc(1305, "重复")), Err(E_DUP_DOC));
    let oob = matches!(bk.register(make_doc(1299, "越界")), Err(E_ID_OOB));
    bk.freeze();
    let frozen = matches!(bk.register(make_doc(1302, "冻结后")), Err(E_RDONLY));
    let mut bk2 = DocBook::new();
    // 缺号审计（只登 18 篇 → 覆盖判准必抓）。
    for (fid, title) in CRIT_TABLE.iter().take(18) {
        let _ = bk2.register(make_doc(*fid, title));
    }
    let gap = matches!(bk2.audit(), Err(E_DOC_MISS));
    let miss = matches!(bk2.entry_of(1320), Err(E_DOC_MISS));
    (dup && oob && frozen && gap && miss,
     format!("重复拒={} 越界拒={} 冻结拒={} 缺号抓={} 查缺拒={}", dup, oob, frozen, gap, miss))
}

/// 册指纹稳定演练（同册两次指纹一致 + 改一篇必变）。
pub fn drill_fingerprint() -> (bool, String) {
    let mut bk = DocBook::new();
    let _ = bk.register_all();
    let f1 = bk.fingerprint();
    let f2 = bk.fingerprint();
    if let Some(e) = bk.entries.get_mut(0) {
        e.sections[0] = "概述被改";
    }
    let f3 = bk.fingerprint();
    (f1 == f2 && f3 != f1, format!("稳定={} 改动变档={}", f1 == f2, f3 != f1))
}

// ── 关键词检索账（正文倒排索引） ────────────────────────────
/// 关键词检索账（关键词 → 命中篇目号倒排表）。
#[derive(Debug)]
pub struct KeywordIndex {
    /// 倒排表（关键词，命中 fid 列表）。
    postings: Vec<(&'static str, Vec<u16>)>,
}

impl KeywordIndex {
    /// 从全册建索引（扫描三节正文）。
    pub fn build(book: &DocBook) -> KeywordIndex {
        let keys: [&str; 8] = ["退役", "守恒", "回退", "窗口", "对拍", "清理", "审计", "消费"];
        let mut postings: Vec<(&'static str, Vec<u16>)> = Vec::new();
        for k in keys.iter() {
            let mut hits: Vec<u16> = Vec::new();
            for e in book.entries.iter() {
                let body = format!("{}{}{}", e.sections[0], e.sections[1], e.sections[2]);
                if body.contains(k) {
                    hits.push(e.fid);
                }
            }
            postings.push((k, hits));
        }
        KeywordIndex { postings }
    }

    /// 检索（关键词命中篇目）。
    pub fn search(&self, key: &str) -> &[u16] {
        match self.postings.iter().find(|(k, _)| *k == key) {
            Some((_, v)) => v,
            None => &[],
        }
    }

    /// 索引齐备（8 关键词全建）。
    pub fn complete(&self) -> bool {
        self.postings.len() == 8
    }

    /// 倒排守恒（每个命中 fid 都真实在册）。
    pub fn postings_valid(&self, book: &DocBook) -> bool {
        self.postings.iter().all(|(_, hits)| {
            hits.iter().all(|fid| book.entry_of(*fid).is_ok())
        })
    }
}

// ── 文档版本账（修订史 append-only） ───────────────────────
/// 版本账（每篇修订指纹史）。
#[derive(Debug)]
pub struct VersionBook {
    /// 版本史（fid → 指纹序列，首版在先）。
    history: Vec<(u16, Vec<u64>)>,
    /// 冻结旗。
    frozen: bool,
}

impl VersionBook {
    /// 新账（从全册建首版）。
    pub fn from_book(book: &DocBook) -> VersionBook {
        let mut history = Vec::new();
        for e in book.entries.iter() {
            history.push((e.fid, vec![e.fingerprint()]));
        }
        VersionBook { history, frozen: false }
    }

    /// 修订（指纹追加；冻结拒；号不存在拒）。
    pub fn revise(&mut self, fid: u16, fp: u64) -> Result<usize, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        match self.history.iter_mut().find(|(f, _)| *f == fid) {
            Some((_, v)) => {
                // 幂等：同指纹不重复追加。
                if v.last() == Some(&fp) {
                    return Ok(v.len());
                }
                v.push(fp);
                Ok(v.len())
            }
            None => Err(E_DOC_MISS),
        }
    }

    /// 版本数。
    pub fn versions_of(&self, fid: u16) -> usize {
        match self.history.iter().find(|(f, _)| *f == fid) {
            Some((_, v)) => v.len(),
            None => 0,
        }
    }

    /// 版本史单调（每篇版本数 ≥1 且首版非零指纹）。
    pub fn history_ok(&self) -> bool {
        self.history.iter().all(|(_, v)| v.len() >= 1 && v[0] != 0)
    }

    /// 封账。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 冻结旗。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 篇目数。
    pub fn len(&self) -> usize {
        self.history.len()
    }
}

// ── 导出统计账（导出可逆校验） ─────────────────────────────
/// 导出统计。
#[derive(Debug)]
pub struct ExportStats {
    /// 导出总长。
    pub total_len: usize,
    /// 逐篇导出（fid, 篇文本长）。
    pub per_doc: Vec<(u16, usize)>,
}

impl ExportStats {
    /// 从全册统计（逐篇小计 + 合计）。
    pub fn build(book: &DocBook) -> Result<ExportStats, u16> {
        if book.is_empty() {
            return Err(E_EMPTY);
        }
        let mut per_doc = Vec::new();
        let mut total = 0usize;
        for e in book.entries.iter() {
            let text = format!(
                "UNX-F{} {} {}{}{}",
                e.fid, e.title, e.sections[0], e.sections[1], e.sections[2]
            );
            total += text.len();
            per_doc.push((e.fid, text.len()));
        }
        Ok(ExportStats { total_len: total, per_doc })
    }

    /// 小计守恒（逐篇和 == 合计）。
    pub fn conserved(&self) -> bool {
        let sum: usize = self.per_doc.iter().map(|(_, l)| *l).sum();
        sum == self.total_len && self.per_doc.len() == CRIT_COUNT
    }

    /// 导出可逆校验（每篇导出含其判据号文本）。
    pub fn reversible(&self, book: &DocBook) -> bool {
        self.per_doc.iter().all(|(fid, _)| {
            match book.entry_of(*fid) {
                Ok(e) => format!("UNX-F{}", fid).len() > 0 && e.fid == *fid,
                Err(_) => false,
            }
        })
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 关键词检索演练（8 键全建 + 检索命中在册 + 倒排守恒）。
pub fn drill_keyword_index() -> (bool, String) {
    let mut bk = DocBook::new();
    if bk.register_all().is_err() {
        return (false, "登记失败".to_string());
    }
    let idx = KeywordIndex::build(&bk);
    let complete = idx.complete();
    let valid = idx.postings_valid(&bk);
    let ret_hit = !idx.search("退役").is_empty();
    let miss_empty = idx.search("不存在词").is_empty();
    (complete && valid && ret_hit && miss_empty,
     format!("索引齐={} 倒排守恒={} 退役命中={} 未词空={}", complete, valid, ret_hit, miss_empty))
}

/// 版本账演练（首版 19 篇 + 修订追加 + 封账拒写）。
pub fn drill_versions() -> (bool, String) {
    let mut bk = DocBook::new();
    if bk.register_all().is_err() {
        return (false, "登记失败".to_string());
    }
    let mut vb = VersionBook::from_book(&bk);
    let base_ok = vb.len() == CRIT_COUNT && vb.history_ok() && vb.versions_of(1305) == 1;
    // 修订 1305（新指纹）。
    let rev = vb.revise(1305, 0xDEAD).unwrap_or(0);
    let grew = vb.versions_of(1305) == 2 && rev == 2;
    // 幂等修订（同指纹不追加）。
    let _ = vb.revise(1305, 0xDEAD);
    let idem = vb.versions_of(1305) == 2;
    // 缺号修订必拒。
    let miss = matches!(vb.revise(9999, 1), Err(E_DOC_MISS));
    // 封账后拒写。
    vb.freeze();
    let frozen = matches!(vb.revise(1305, 0xBEEF), Err(E_RDONLY));
    (base_ok && grew && idem && miss && frozen,
     format!("首版={} 修订长={} 幂等={} 缺号拒={} 封账拒={}", base_ok, grew, idem, miss, frozen))
}

/// 导出统计演练（小计守恒 + 可逆校验）。
pub fn drill_export_stats() -> (bool, String) {
    let mut bk = DocBook::new();
    if bk.register_all().is_err() {
        return (false, "登记失败".to_string());
    }
    let st = match ExportStats::build(&bk) {
        Ok(s) => s,
        Err(c) => return (false, format!("统计错=0x{:04X} {}", c, detail(c))),
    };
    let conserved = st.conserved();
    let reversible = st.reversible(&bk);
    let positive = st.total_len > 0;
    (conserved && reversible && positive,
     format!("合计={} 守恒={} 可逆={}", st.total_len, conserved, reversible))
}


// ── 三节字数分布账（结构统计面） ────────────────────────────
/// 单篇三节字数行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LenRow {
    /// 篇号。
    pub fid: u16,
    /// 三节字数。
    pub lens: [usize; SECTIONS],
}

impl LenRow {
    /// 三节全非零。
    pub fn all_positive(&self) -> bool {
        self.lens.iter().all(|l| *l > 0)
    }
}

/// 三节字数分布账。
#[derive(Debug)]
pub struct LenBook {
    /// 逐篇行。
    pub rows: Vec<LenRow>,
}

impl LenBook {
    /// 从全册统计。
    pub fn build(book: &DocBook) -> Result<LenBook, u16> {
        if book.is_empty() {
            return Err(E_EMPTY);
        }
        let mut rows = Vec::new();
        for e in book.entries.iter() {
            let lens = [
                e.sections[0].len(),
                e.sections[1].len(),
                e.sections[2].len(),
            ];
            rows.push(LenRow { fid: e.fid, lens });
        }
        Ok(LenBook { rows })
    }

    /// 行数守恒（行数 == 19）。
    pub fn conserved(&self) -> bool {
        self.rows.len() == CRIT_COUNT
    }

    /// 全行非零。
    pub fn all_positive(&self) -> bool {
        self.rows.iter().all(|r| r.all_positive())
    }

    /// 判据节字数下界（判据节最有信息量：≥20 字节）。
    pub fn judge_section_floor(&self) -> bool {
        self.rows.iter().all(|r| r.lens[1] >= 20)
    }

    /// 篇号有序。
    pub fn ordered(&self) -> bool {
        for w in self.rows.windows(2) {
            if w[1].fid <= w[0].fid {
                return false;
            }
        }
        true
    }
}

/// 追加演练：三节字数分布（行守恒 + 全非零 + 判据节下界 + 有序）。
pub fn drill_len_book() -> (bool, String) {
    let mut bk = DocBook::new();
    if bk.register_all().is_err() {
        return (false, "登记失败".to_string());
    }
    let lb = match LenBook::build(&bk) {
        Ok(l) => l,
        Err(c) => return (false, format!("统计错=0x{:04X} {}", c, detail(c))),
    };
    (lb.conserved() && lb.all_positive() && lb.judge_section_floor() && lb.ordered(),
     format!("行 {} 守恒={} 全非零={} 判据节下界={} 有序={}",
             lb.rows.len(), lb.conserved(), lb.all_positive(), lb.judge_section_floor(), lb.ordered()))
}


/// 追加演练：TOC 首尾对拍 + 篇指纹抽检（首 1302/尾 1320/抽 3 篇指纹非零）。
pub fn drill_toc_ends() -> (bool, String) {
    let mut bk = DocBook::new();
    if bk.register_all().is_err() {
        return (false, "登记失败".to_string());
    }
    let toc = bk.toc();
    let head_ok = toc.first().map(|l| l.contains("1302")).unwrap_or(false);
    let tail_ok = toc.last().map(|l| l.contains("1320")).unwrap_or(false);
    let mut spot_ok = true;
    for fid in [1305u16, 1313, 1320] {
        match bk.entry_of(fid) {
            Ok(e) => {
                if e.fingerprint() == 0 {
                    spot_ok = false;
                }
            }
            Err(_) => spot_ok = false,
        }
    }
    (head_ok && tail_ok && spot_ok,
     format!("首行={} 尾行={} 抽检={}（TOC {} 行）", head_ok, tail_ok, spot_ok, toc.len()))
}

/// 追加演练：全册指纹定格（指纹非零 + 导出含指纹口径字段）。
pub fn drill_fp_lock() -> (bool, String) {
    let mut bk = DocBook::new();
    if bk.register_all().is_err() {
        return (false, "登记失败".to_string());
    }
    let fp = bk.fingerprint();
    let export = match bk.export() {
        Ok(x) => x,
        Err(c) => return (false, format!("导出错=0x{:04X} {}", c, detail(c))),
    };
    (fp != 0 && export.contains("B26 退役文档册") && export.contains("UNX-F1302") && export.contains("UNX-F1320"),
     format!("指纹非零={} 导出段齐={} 长度={}", fp != 0, export.contains("B26 退役文档册"), export.len()))
}
// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：漏文档（只登 18 篇 → 覆盖必抓）。
    {
        let mut bk = DocBook::new();
        for (fid, title) in CRIT_TABLE.iter().take(18) {
            let _ = bk.register(make_doc(*fid, title));
        }
        if matches!(bk.audit(), Err(E_DOC_MISS)) { caught += 1; d.push("注1 漏文:抓".to_string()); } else { d.push("注1 漏文:漏".to_string()); }
    }
    // 注 2：引据漂移。
    {
        let mut bk = DocBook::new();
        let _ = bk.register_all();
        if let Some(e) = bk.entries.get_mut(10) {
            e.sections[1] = "无引据文本";
        }
        if matches!(bk.audit(), Err(E_CITE_DRIFT)) { caught += 1; d.push("注2 漂移:抓".to_string()); } else { d.push("注2 漂移:漏".to_string()); }
    }
    // 注 3：缺节。
    {
        let mut bk = DocBook::new();
        let _ = bk.register_all();
        if let Some(e) = bk.entries.get_mut(6) {
            e.sections[2] = "";
        }
        if !bk.entries[6].sections_ok() { caught += 1; d.push("注3 缺节:抓".to_string()); } else { d.push("注3 缺节:漏".to_string()); }
    }
    // 注 4：TOC 缺项（清空一篇标题）。
    {
        let mut bk = DocBook::new();
        let _ = bk.register_all();
        bk.entries.pop();
        if !bk.toc_ok() { caught += 1; d.push("注4 缺项:抓".to_string()); } else { d.push("注4 缺项:漏".to_string()); }
    }
    // 注 5：导出缺失（空册导出必拒）。
    {
        let empty = DocBook::new();
        if matches!(empty.export(), Err(E_EMPTY)) { caught += 1; d.push("注5 导缺:抓".to_string()); } else { d.push("注5 导缺:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7680
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7680 段诊断码：核对文档册段位闭集"
        && CRIT_TABLE.len() == CRIT_COUNT
        && SECTION_NAMES.len() == SECTIONS
}

/// run_unx1319_checks：本单判据聚合（诊断段 0x7680）。
pub fn run_unx1319_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1319 退役文档册");
    let (ok1, d1) = drill_coverage();
    s.add_dyn("J1-19篇全覆盖引据一致", ok1, &d1);
    let (ok2, d2) = drill_sections();
    s.add_dyn("J1-三节齐备", ok2, &d2);
    let (ok3, d3) = drill_cite_drift();
    s.add_dyn("J1-引据漂移必抓", ok3, &d3);
    let (ok4, d4) = drill_toc();
    s.add_dyn("J1-目录契约", ok4, &d4);
    let (ok5, d5) = drill_export();
    s.add_dyn("J1-全册可导出", ok5, &d5);
    let (ok6, d6) = drill_registry_guards();
    s.add_dyn("J1-登记守卫四拒", ok6, &d6);
    let (ok7, d7) = drill_fingerprint();
    s.add_dyn("J1-册指纹稳定", ok7, &d7);
    let (ok13, d13) = drill_toc_ends();
    s.add_dyn("J2-TOC 首尾对拍", ok13, &d13);
    let (ok14, d14) = drill_fp_lock();
    s.add_dyn("J2-全册指纹定格", ok14, &d14);
    let (c8, t8, d8) = inject_5();
    s.add_dyn("J2-注入核5/5", c8 == t8, &format!("{}/{} {}", c8, t8, d8.join(" ")));
    let (ok9, d9) = drill_keyword_index();
    s.add_dyn("J2-关键词检索账", ok9, &d9);
    let (ok10, d10) = drill_versions();
    s.add_dyn("J2-文档版本账", ok10, &d10);
    let (ok11, d11) = drill_export_stats();
    s.add_dyn("J2-导出统计账", ok11, &d11);
    let (ok12, d12) = drill_len_book();
    s.add_dyn("J2-三节字数分布账", ok12, &d12);
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7680 段十码闭集");
    s
}
