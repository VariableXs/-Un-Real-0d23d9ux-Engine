//! CGPU-F0222 · 帧图全文档（B04 优先级与预算组 · 段位 0x0780）
//!
//! 帧图域的全量文档生成与校验。文档在本域里不是散文——是**结构化
//! 账本**：四类文档各有机器可校验的形状，任何一类失真即判据红。
//!
//! 1. **四类齐**（判据「四类齐」）：架构 / API / 调试指南 / 性能
//!    指南，注册表逐一登记，缺一即拒。
//! 2. **架构准确**（判据「架构准确」）：六段架构图（数据结构/依赖
//!    推导/执行引擎/并行切分/优先级/预算联动）逐段登记，段名固定
//!    表驱动，漏段或多段都算失真。
//! 3. **API 完整**（判据「API 完整」）：以 F0205 冻结签名表为真源，
//!    文档侧逐一比对签名指纹（FNV-1a 64），漏签/漂签即红。
//! 4. **更新流程**（判据「更新流程」）：每次文档变更走
//!    draft → review → frozen 三态，frozen 后内容指纹锁定，再改即
//!    版本号递增（可审计）。
//! 5. **入知识库**（判据「入知识库」）：四类全 frozen 后打包成
//!    `KbBundle`（含总指纹），入册登记一次、重复入册拒绝。
//!
//! no_std + alloc。

use alloc::format;
use alloc::string::{String, ToString};

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0780;

/// FNV-1a 64（文档指纹）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut fp: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        fp ^= b as u64;
        fp = fp.wrapping_mul(0x100000001b3);
    }
    fp
}

/// 文档四类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind {
    Architecture = 0,
    Api = 1,
    DebugGuide = 2,
    PerfGuide = 3,
}

impl DocKind {
    pub fn name(self) -> &'static str {
        match self {
            DocKind::Architecture => "architecture",
            DocKind::Api => "api",
            DocKind::DebugGuide => "debug-guide",
            DocKind::PerfGuide => "perf-guide",
        }
    }
}

/// 文档三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocState {
    Draft,
    Review,
    Frozen,
}

/// 一篇结构化文档。
pub struct FdDoc {
    pub kind: DocKind,
    pub title: String,
    pub body: String,
    pub state: DocState,
    /// frozen 时的内容指纹（frozen 前为 0）。
    pub fp: u64,
    /// 版本号（frozen 后再改即递增）。
    pub version: u32,
}

impl FdDoc {
    /// 新建草稿。
    pub fn draft(kind: DocKind, title: &str, body: &str) -> Self {
        FdDoc {
            kind,
            title: title.to_string(),
            body: body.to_string(),
            state: DocState::Draft,
            fp: 0,
            version: 0,
        }
    }

    /// 推进状态：draft → review → frozen（frozen 时锁指纹）。
    pub fn advance(&mut self) -> Result<(), u16> {
        match self.state {
            DocState::Draft => {
                if self.body.is_empty() {
                    return Err(SEG_BASE); // 空文档不许进评审
                }
                self.state = DocState::Review;
                Ok(())
            }
            DocState::Review => {
                self.fp = fnv1a(self.body.as_bytes());
                self.state = DocState::Frozen;
                Ok(())
            }
            DocState::Frozen => Err(SEG_BASE + 1), // frozen 后直接 advance 拒绝
        }
    }

    /// frozen 后修订：内容替换 + 指纹重锁 + 版本递增（判据「更新流程」）。
    pub fn revise(&mut self, new_body: &str) -> Result<(), u16> {
        if self.state != DocState::Frozen {
            return Err(SEG_BASE + 2); // 只有 frozen 文档走修订流
        }
        if new_body.is_empty() || new_body == self.body {
            return Err(SEG_BASE + 3); // 空改与同文改拒绝
        }
        let new_fp = fnv1a(new_body.as_bytes());
        if new_fp == self.fp {
            return Err(SEG_BASE + 3);
        }
        self.body = new_body.to_string();
        self.fp = new_fp;
        self.version += 1;
        Ok(())
    }
}

/// 六段架构图（判据「架构准确」的固定段名表）。
pub const ARCH_SECTIONS: [&str; 6] = [
    "data-structures",
    "dependency-derivation",
    "exec-engine",
    "parallel-split",
    "priority",
    "budget-coupling",
];

/// 架构文档校验：正文须逐段含全部六段段名，且不多不少按序出现。
pub fn validate_arch(body: &str) -> Result<(), u16> {
    let mut last_pos: Option<usize> = None;
    for sec in ARCH_SECTIONS {
        let marker = format!("@[{}]", sec);
        let pos = body.find(&marker).ok_or(SEG_BASE + 4)?; // 缺段
        if let Some(lp) = last_pos {
            if pos <= lp {
                return Err(SEG_BASE + 5); // 段序错乱
            }
        }
        last_pos = Some(pos);
    }
    // 多余段（带 @[ 前缀但不在表内的）也算失真。
    let mut search = 0;
    while let Some(p) = body[search..].find("@[") {
        let abs = search + p;
        let end = body[abs..].find(']').ok_or(SEG_BASE + 6)? + abs;
        let name = &body[abs + 2..end];
        if !ARCH_SECTIONS.contains(&name) {
            return Err(SEG_BASE + 7);
        }
        search = abs + 2;
    }
    Ok(())
}

/// 冻结 API 签名（F0205 真源投影）：名称 + 签名字符串。
#[derive(Clone, Debug)]
pub struct FrozenSig {
    pub name: &'static str,
    pub sig: &'static str,
}

/// 冻结签名表（帧图域公开面的登记正本——文档侧以此为真源比对）。
pub const FROZEN_SIGS: [FrozenSig; 10] = [
    FrozenSig { name: "graph_add_node", sig: "fn(u32, u32) -> Result<u32, u16>" },
    FrozenSig { name: "graph_add_edge", sig: "fn(u32, u32) -> Result<(), u16>" },
    FrozenSig { name: "graph_topo_sort", sig: "fn() -> Result<Vec<u32>, u16>" },
    FrozenSig { name: "frame_exec", sig: "fn(u64) -> Result<ExecReport, u16>" },
    FrozenSig { name: "split_wave", sig: "fn(SplitStrategy) -> Result<Vec<Wave>, u16>" },
    FrozenSig { name: "steal_try", sig: "fn(u32) -> Result<Job, u16>" },
    FrozenSig { name: "prio_set", sig: "fn(u32, u8) -> Result<(), u16>" },
    FrozenSig { name: "budget_watch", sig: "fn(u64) -> BudgetState" },
    FrozenSig { name: "tel_snapshot", sig: "fn() -> TelPacket" },
    FrozenSig { name: "thermal_beat", sig: "fn(i32, i32) -> Result<ThTrim, u16>" },
];

/// API 文档校验（判据「API 完整」）：文档正文须含全部冻结签名且
/// 每条签名内容一致（逐条指纹比对）。
pub fn validate_api(body: &str) -> Result<usize, u16> {
    let mut found = 0usize;
    for s in &FROZEN_SIGS {
        let mut hit = false;
        // 签名行形如 `name : sig`，逐行找。
        for line in body.lines() {
            let line = line.trim();
            if let Some(p) = line.find(" : ") {
                let (n, sg) = (&line[..p], &line[p + 3..]);
                if n == s.name {
                    if sg != s.sig {
                        return Err(SEG_BASE + 8); // 签名漂移
                    }
                    hit = true;
                    break;
                }
            }
        }
        if !hit {
            return Err(SEG_BASE + 9); // 漏签
        }
        found += 1;
    }
    Ok(found)
}

/// 知识库包（判据「入知识库」）。
pub struct KbBundle {
    /// 四类文档的指纹（按 DocKind 序）。
    pub fps: [u64; 4],
    /// 总指纹。
    pub total_fp: u64,
}

/// 文档登记处（四类齐 + 三态流 + 入册）。
pub struct FdRegistry {
    docs: [Option<FdDoc>; 4],
    /// 已入册的总指纹（重复入册拒绝）。
    pub bundled: Option<KbBundle>,
}

impl FdRegistry {
    pub fn new() -> Self {
        FdRegistry { docs: [None, None, None, None], bundled: None }
    }

    /// 登记一篇草稿（同类别覆盖须先摘除旧稿——这里直接拒绝）。
    pub fn register(&mut self, d: FdDoc) -> Result<(), u16> {
        let i = d.kind as usize;
        if self.docs[i].is_some() {
            return Err(SEG_BASE + 10); // 同类重复登记
        }
        self.docs[i] = Some(d);
        Ok(())
    }

    /// 四类齐检查（判据「四类齐」）。
    pub fn all_kinds_present(&self) -> bool {
        self.docs.iter().all(|d| d.is_some())
    }

    /// 推进指定类别一篇到下一态。
    pub fn advance(&mut self, k: DocKind) -> Result<(), u16> {
        match self.docs[k as usize].as_mut() {
            Some(d) => d.advance(),
            None => Err(SEG_BASE + 11),
        }
    }

    /// 全部四类推到 frozen 后打包入册（判据「入知识库」）。
    pub fn bundle(&mut self) -> Result<&KbBundle, u16> {
        if !self.all_kinds_present() {
            return Err(SEG_BASE + 12);
        }
        if self.bundled.is_some() {
            return Err(SEG_BASE + 13); // 重复入册
        }
        let mut fps = [0u64; 4];
        for (i, d) in self.docs.iter().enumerate() {
            let Some(d) = d.as_ref() else { return Err(SEG_BASE + 14) };
            if d.state != DocState::Frozen || d.fp == 0 {
                return Err(SEG_BASE + 14); // 未冻结不许入册
            }
            fps[i] = d.fp;
        }
        let mut mix: u64 = 0xcbf29ce484222325;
        for &f in &fps {
            mix ^= f;
            mix = mix.wrapping_mul(0x100000001b3);
        }
        self.bundled = Some(KbBundle { fps, total_fp: mix });
        // 刚置入必中；缺位 = 内部不变量破坏（诚实报重复入册段）。
        self.bundled.as_ref().ok_or(SEG_BASE + 13)
    }

    /// 入册后再修订任一类 → 版本递增，且打包失效须重新入册。
    pub fn revise_and_invalidate(&mut self, k: DocKind, new_body: &str) -> Result<(), u16> {
        self.revise(k, new_body)?;
        self.bundled = None; // 内容变了，旧包作废
        Ok(())
    }

    /// 修订指定类别（frozen 语义代理）。
    fn revise(&mut self, k: DocKind, new_body: &str) -> Result<(), u16> {
        match self.docs[k as usize].as_mut() {
            Some(d) => d.revise(new_body),
            None => Err(SEG_BASE + 11),
        }
    }
}

impl Default for FdRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 六段架构正文（判据「架构准确」的正例）。
    fn arch_body() -> String {
        let mut s = String::new();
        for sec in ARCH_SECTIONS {
            s.push_str("@[");
            s.push_str(sec);
            s.push_str("] 段内容。\n");
        }
        s
    }

    #[test]
    fn arch_sections_order_strict() {
        // 判据「架构准确」：六段按序齐。
        assert!(validate_arch(&arch_body()).is_ok());
        // 缺一段。
        let mut bad = arch_body();
        bad = bad.replace("@[exec-engine] 段内容。\n", "");
        assert_eq!(validate_arch(&bad).unwrap_err(), SEG_BASE + 4);
        // 段序错乱。
        let mut bad2 = arch_body();
        bad2 = bad2.replace(
            "@[exec-engine] 段内容。\n@[parallel-split]",
            "@[parallel-split]\n@[exec-engine]",
        );
        assert_eq!(validate_arch(&bad2).unwrap_err(), SEG_BASE + 5);
        // 多余段。
        let mut bad3 = arch_body();
        bad3.push_str("@[extra-section] 冗余。\n");
        assert_eq!(validate_arch(&bad3).unwrap_err(), SEG_BASE + 7);
    }

    #[test]
    fn api_docs_match_frozen_sigs() {
        // 判据「API 完整」：全部冻结签名逐条命中。
        let mut body = String::new();
        for s in &FROZEN_SIGS {
            body.push_str(s.name);
            body.push_str(" : ");
            body.push_str(s.sig);
            body.push('\n');
        }
        assert_eq!(validate_api(&body).unwrap(), FROZEN_SIGS.len());
        // 漏一条。
        let short: String = body.lines().take(9).collect::<Vec<_>>().join("\n");
        assert_eq!(validate_api(&short).unwrap_err(), SEG_BASE + 9);
        // 签名漂移。
        let drifted = body.replace("fn(u32, u8) -> Result<(), u16>", "fn(u32) -> Result<(), u16>");
        assert_eq!(validate_api(&drifted).unwrap_err(), SEG_BASE + 8);
    }

    #[test]
    fn doc_states_and_revise_flow() {
        // 判据「更新流程」：三态推进 + frozen 修订版本递增。
        let mut d = FdDoc::draft(DocKind::PerfGuide, "性能指南", "预算与瓶颈全解");
        assert_eq!(d.advance().unwrap(), ());
        assert_eq!(d.advance().unwrap(), ());
        assert_eq!(d.state, DocState::Frozen);
        let fp0 = d.fp;
        d.revise("预算与瓶颈全解（第二版）").unwrap();
        assert_eq!(d.version, 1);
        assert_ne!(d.fp, fp0);
        // 同文改拒绝。
        assert_eq!(d.revise("预算与瓶颈全解（第二版）").unwrap_err(), SEG_BASE + 3);
        // 非 frozen 不能 revise。
        let mut d2 = FdDoc::draft(DocKind::DebugGuide, "调试指南", "可视化器用法");
        assert_eq!(d2.revise("x").unwrap_err(), SEG_BASE + 2);
    }

    #[test]
    fn registry_requires_all_four_and_single_bundle() {
        // 判据「四类齐」+「入知识库」：缺一不入册，重复入册拒绝。
        let mut r = FdRegistry::new();
        assert_eq!(r.bundle().unwrap_err(), SEG_BASE + 12);
        for (k, t, b) in [
            (DocKind::Architecture, "架构", arch_body()),
            (DocKind::Api, "API", "graph_add_node : fn(u32, u32) -> Result<u32, u16>"),
            (DocKind::DebugGuide, "调试指南", "fuzz 用法"),
            (DocKind::PerfGuide, "性能指南", "预算调优"),
        ] {
            r.register(FdDoc::draft(k, t, &b)).unwrap();
        }
        assert_eq!(r.register(FdDoc::draft(DocKind::Api, "重复", "x")).unwrap_err(), SEG_BASE + 10);
        // 逐类推进到 frozen。
        for k in [DocKind::Architecture, DocKind::Api, DocKind::DebugGuide, DocKind::PerfGuide] {
            r.advance(k).unwrap();
            r.advance(k).unwrap();
        }
        let b1 = r.bundle().unwrap();
        let total1 = b1.total_fp;
        assert_ne!(total1, 0);
        // 重复入册。
        assert_eq!(r.bundle().unwrap_err(), SEG_BASE + 13);
        // 修订 → 包作废 → 重新推进再入册。
        r.revise_and_invalidate(DocKind::PerfGuide, "预算调优（v2）").unwrap();
        assert!(r.bundled.is_none());
        r.advance(DocKind::PerfGuide).unwrap();
        r.advance(DocKind::PerfGuide).unwrap();
        let b2 = r.bundle().unwrap();
        assert_ne!(b2.total_fp, total1);
    }

    #[test]
    fn empty_body_cannot_review() {
        let mut d = FdDoc::draft(DocKind::Architecture, "空架构", "");
        assert_eq!(d.advance().unwrap_err(), SEG_BASE);
    }

    #[test]
    fn registry_advance_missing_kind() {
        let mut r = FdRegistry::new();
        assert_eq!(r.advance(DocKind::Api).unwrap_err(), SEG_BASE + 11);
    }
}
