#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0796 · 后续维护通道声明（收官后勘误/升级/重验/回执四路径）
//! 域/批：CoRun·UNX A1 收官域（维护/封域面 · W7 清单第 23 单）
//! 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥580 行；② 判据原文：UNX-F0796-J1 通道声明齐（勘误/升级/
//!   重验/回执四路径 + 入口文件清单 = 4/4）且逐路径有流程引用；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 四路径闭集建模：勘误（Errata）/ 升级（Upgrade）/ 重验（Reverify）/
//!   回执（Receipt）四通道，各通道 = 触发条件文本（非空）+ 流程引用（入口
//!   文件清单中的文件 ID，可解引）+ 责任档位（Owner/Delegate 闭集）。
//! ② 入口文件清单：文件 ID 闭集登记簿（先登记后解引），文件 ID 唯一、
//!   锚点非空；四路径的流程引用逐条解引成功，未登记即红。
//! ③ 守恒恒等式：路径数 = 4 = 四通道各恰好 1 条（不重不漏）；
//!   清单条目数 = 被解引数 + 未用数。
//! ④ 状态机 Open→Declared→Resolved→Filed 单向闭集；诊断码定位到路径号。
//! ⑤ 逐路径流程引用非空 + 责任档位非空；摘要非空。
//! 诊断段声明：0x61C0–0x61DF 本单独占（清单 <SEG 0x61C0>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x61C0–0x61DF）
pub const SEG_BASE: u16 = 0x61C0; // 诊断码段基数，来源：W7 清单 UNX-F0796 行 <SEG 0x61C0>
pub const SEG_TOP: u16 = 0x61DF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 通道声明状态机非法边
pub const XC_DUP_PATH: u16 = SEG_BASE + 2; // 通道重复（四路径不重破）
pub const XC_PATH_OUT: u16 = SEG_BASE + 3; // 通道越出四路径闭集
pub const XC_TRIG_EMPTY: u16 = SEG_BASE + 4; // 触发条件文本为空
pub const XC_REF_EMPTY: u16 = SEG_BASE + 5; // 流程引用为空
pub const XC_FILE_UNKNOWN: u16 = SEG_BASE + 6; // 流程引用未在入口清单登记
pub const XC_FILE_DUP: u16 = SEG_BASE + 7; // 入口文件 ID 重复
pub const XC_FILE_SHORT: u16 = SEG_BASE + 8; // 入口清单不足 4 文件
pub const XC_OWNER_INVALID: u16 = SEG_BASE + 9; // 责任档位非法
pub const XC_NOT_ALL_PATH: u16 = SEG_BASE + 10; // 存在路径无流程引用
pub const XC_ALREADY_FILED: u16 = SEG_BASE + 11; // 已归档再归档
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 12; // 摘要为空
pub const XC_CONSERV_BREAK: u16 = SEG_BASE + 13; // 守恒恒等式破
pub const XC_PATH_SHORT: u16 = SEG_BASE + 14; // 声明路径不足 4 条

// 2. 维护常量（写死，来源=判据原文/本单口径头注假设）
pub const PATH_N: usize = 4; // 四路径（勘误/升级/重验/回执），来源：判据原文
pub const FILE_MIN: usize = 4; // 入口文件清单下限（每路径至少一个入口），头注假设
pub const FILE_CAP: usize = 16; // 入口清单定容，头注假设

/// 四路径闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaintPath {
    Errata = 0, // 勘误通道
    Upgrade = 1, // 升级通道
    Reverify = 2, // 重验通道
    Receipt = 3, // 回执通道
}
pub const PATH_ALL: [MaintPath; 4] = [MaintPath::Errata, MaintPath::Upgrade, MaintPath::Reverify, MaintPath::Receipt];

/// 责任档位闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    Chief = 0, // 主责
    Delegate = 1, // 受托
}
impl Owner {
    pub fn view(self) -> &'static str {
        match self {
            Owner::Chief => "chief",
            Owner::Delegate => "delegate",
        }
    }
}

/// 一条通道声明。
#[derive(Clone, Debug)]
pub struct PathDecl {
    pub pid: u32, // 路径号 1..=4（唯一）
    pub path: MaintPath, // 通道（闭集，不重）
    pub trigger: String, // 触发条件文本（非空）
    pub flow_ref: String, // 流程引用（入口文件 ID，可解引）
    pub owner: Owner, // 责任档位
}

/// 入口文件清单条目。
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub fid: String, // 文件 ID（唯一键）
    pub anchor: String, // 锚点说明（文件名/用途，非空）
}

/// 归档回执。
#[derive(Clone, Debug)]
pub struct MaintFiled {
    pub paths: u32, // 声明路径数（4）
    pub files: u32, // 入口清单条目数
    pub all_flow: bool, // 逐路径有流程引用结论
    pub summary: String, // 摘要（非空）
    pub fingerprint: u64, // 声明账指纹（对拍用）
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaintStage {
    Open = 0,
    Declared = 1,
    Resolved = 2,
    Filed = 3,
}
impl MaintStage {
    pub fn can_step(self, next: MaintStage) -> bool {
        (self as u8) + 1 == next as u8
    }
    pub fn view(self) -> &'static str {
        match self {
            MaintStage::Open => "open",
            MaintStage::Declared => "declared",
            MaintStage::Resolved => "resolved",
            MaintStage::Filed => "filed",
        }
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步（字节混合）。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub fn fnv_str(seed: u64, s: &str) -> u64 {  // 字符串 FNV（逐字节）。
    let mut h = seed;
    let bs = s.as_bytes();
    let mut i = 0usize;
    while i < bs.len() {
        h ^= bs[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}
pub struct MaintBoard {  // 维护通道声明板。
    stage: MaintStage,
    decls: Vec<PathDecl>,
    files: Vec<FileEntry>,
    resolved: Vec<u32>, // 已逐条解引成功的路径号
    pinned: Option<MaintFiled>,
    events: Vec<String>,
}
impl MaintBoard {
    pub fn new() -> Self {
        MaintBoard {
            stage: MaintStage::Open,
            decls: Vec::new(),
            files: Vec::new(),
            resolved: Vec::new(),
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> MaintStage {
        self.stage
    }
    pub fn decl_len(&self) -> usize {
        self.decls.len()
    }
    pub fn file_len(&self) -> usize {
        self.files.len()
    }
    pub fn resolved_len(&self) -> usize {
        self.resolved.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn register_file(&mut self, e: FileEntry) -> Result<u32, u16> {  // 入口文件登记：ID 唯一、锚点非空、容量守卫。
        if self.stage != MaintStage::Open && self.stage != MaintStage::Declared {
            return Err(XC_STATE_INVALID);
        }
        if e.fid.is_empty() {
            return Err(XC_REF_EMPTY);
        }
        if e.anchor.is_empty() {
            return Err(XC_TRIG_EMPTY);
        }
        if self.files.len() >= FILE_CAP {
            return Err(XC_CONSERV_BREAK);
        }
        let mut i = 0usize;
        while i < self.files.len() {
            if self.files[i].fid == e.fid {
                return Err(XC_FILE_DUP);
            }
            i += 1;
        }
        self.files.push(e);
        self.events.push(alloc::format!("file +{}", self.files.len()));
        Ok(self.files.len() as u32)
    }
    pub fn declare(&mut self, d: PathDecl) -> Result<u32, u16> {  // 通道声明：号唯一、通道闭集不重、触发/引用非空。
        if self.stage != MaintStage::Open && self.stage != MaintStage::Declared {
            return Err(XC_STATE_INVALID);
        }
        if d.pid < 1 || d.pid as usize > PATH_N {
            return Err(XC_PATH_OUT);
        }
        if d.trigger.is_empty() {
            return Err(XC_TRIG_EMPTY);
        }
        if d.flow_ref.is_empty() {
            return Err(XC_REF_EMPTY);
        }
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].pid == d.pid {
                return Err(XC_FILE_DUP); // 复用重号码：路径号重复
            }
            if self.decls[i].path == d.path {
                return Err(XC_DUP_PATH); // 四路径不重
            }
            i += 1;
        }
        self.decls.push(d);
        self.stage = MaintStage::Declared;
        self.events.push(alloc::format!("decl {}", self.decls.len()));
        Ok(self.decls.len() as u32)
    }
    pub fn resolve(&mut self) -> Result<u32, u16> {  // 逐条解引：四条流程引用逐条到入口清单解引，未登记即红并定位。
        if !self.stage.can_step(MaintStage::Resolved) {
            return Err(XC_STATE_INVALID);
        }
        if self.decls.len() != PATH_N {
            return Err(XC_PATH_SHORT);
        }
        if self.files.len() < FILE_MIN {
            return Err(XC_FILE_SHORT);
        }
        self.resolved.clear();
        let mut k = 0usize;
        while k < self.decls.len() {
            let d = &self.decls[k];
            let mut hit = false;
            let mut i = 0usize;
            while i < self.files.len() {
                if self.files[i].fid == d.flow_ref {
                    hit = true;
                    break;
                }
                i += 1;
            }
            if !hit {
                return Err(XC_FILE_UNKNOWN); // 定位：k 即路径下标
            }
            self.resolved.push(d.pid);
            k += 1;
        }
        self.stage = MaintStage::Resolved;
        self.events.push(alloc::format!("resolved {}", self.resolved.len()));
        Ok(self.resolved.len() as u32)
    }
    pub fn file_it(&mut self) -> Result<MaintFiled, u16> {  // 归档：4 路径全解引 → 摘要定格（重归档红）。
        if !self.stage.can_step(MaintStage::Filed) {
            return Err(if self.pinned.is_some() {
                XC_ALREADY_FILED
            } else {
                XC_STATE_INVALID
            });
        }
        if self.resolved.len() != PATH_N {
            return Err(XC_NOT_ALL_PATH);
        }
        // 守恒恒等式：被解引文件数 = 4 ≤ 清单总数；used + unused = 总数。
        let mut used = 0usize;
        let mut a = 0usize;
        while a < self.files.len() {
            let mut is_used = false;
            let mut b = 0usize;
            while b < self.decls.len() {
                if self.decls[b].flow_ref == self.files[a].fid {
                    is_used = true;
                    break;
                }
                b += 1;
            }
            if is_used {
                used += 1;
            }
            a += 1;
        }
        if used != PATH_N {
            return Err(XC_CONSERV_BREAK);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0796-J1 通道声明齐：勘误/升级/重验/回执四路径 + 入口文件清单（{} 文件）逐路径流程引用解引 4/4",
            self.files.len()
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = MaintFiled {
            paths: self.decls.len() as u32,
            files: self.files.len() as u32,
            all_flow: true,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = MaintStage::Filed;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&MaintFiled> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 声明账指纹：声明（pid,path,flow_ref）+ 清单（fid）规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.decls.len() {
            h = fnv1a(h, self.decls[i].pid as u64);
            h = fnv1a(h, self.decls[i].path as u64);
            h = fnv_str(h, &self.decls[i].flow_ref);
            i += 1;
        }
        i = 0;
        while i < self.files.len() {
            h = fnv_str(h, &self.files[i].fid);
            i += 1;
        }
        h
    }
    pub fn decl_of(&self, pid: u32) -> Option<&PathDecl> {  // 声明查询。
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].pid == pid {
                return Some(&self.decls[i]);
            }
            i += 1;
        }
        None
    }
    pub fn path_of(&self, p: MaintPath) -> Option<&PathDecl> {  // 按通道查声明（四通道各恰一条）。
        let mut i = 0usize;
        while i < self.decls.len() {
            if self.decls[i].path == p {
                return Some(&self.decls[i]);
            }
            i += 1;
        }
        None
    }
    pub fn file_of(&self, fid: &str) -> Option<&FileEntry> {  // 入口文件查询。
        let mut i = 0usize;
        while i < self.files.len() {
            if self.files[i].fid == fid {
                return Some(&self.files[i]);
            }
            i += 1;
        }
        None
    }
    pub fn decl_table_view(&self) -> String {  // 声明视图串："pid|owner|flow_ref" 行串。
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.decls.len() {
            let d = &self.decls[i];
            s.push_str(alloc::format!("{}|{}|{}\n", d.pid, d.owner.view(), d.flow_ref).as_str());
            i += 1;
        }
        s
    }
    pub fn unused_files(&self) -> Vec<String> {  // 未被任何声明解引的入口文件（守恒：used + unused = 总数）。
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.files.len() {
            let mut used = false;
            let mut a = 0usize;
            while a < self.decls.len() {
                if self.decls[a].flow_ref == self.files[i].fid {
                    used = true;
                    break;
                }
                a += 1;
            }
            if !used {
                out.push(self.files[i].fid.clone());
            }
            i += 1;
        }
        out
    }
    pub fn owner_histogram(&self) -> [u32; 2] {  // 责任档位直方图 [Chief, Delegate]（守恒=声明总数）。
        let mut h = [0u32; 2];
        let mut i = 0usize;
        while i < self.decls.len() {
            h[self.decls[i].owner as usize] += 1;
            i += 1;
        }
        h
    }
}
pub fn gen_decls() -> Vec<PathDecl> {  // 样本发生器：四路径各一条声明（确定性文本与流程引用）。
    let mut out: Vec<PathDecl> = Vec::new();
    let mut i = 0usize;
    while i < PATH_N {
        let p = PATH_ALL[i];
        out.push(PathDecl {
            pid: (i + 1) as u32,
            path: p,
            trigger: alloc::format!("触发_{:?}_条件成立时走本通道", p),
            flow_ref: alloc::format!("FL-A1-{:02}", i + 1),
            owner: if i % 2 == 0 { Owner::Chief } else { Owner::Delegate },
        });
        i += 1;
    }
    out
}
pub fn gen_files() -> Vec<FileEntry> {  // 样本发生器：入口清单 5 条（4 被用 + 1 未用，确定性）。
    let mut out: Vec<FileEntry> = Vec::new();
    let mut i = 0usize;
    while i < PATH_N {
        out.push(FileEntry {
            fid: alloc::format!("FL-A1-{:02}", i + 1),
            anchor: alloc::format!("入口文件_{:02}_流程锚点", i + 1),
        });
        i += 1;
    }
    out.push(FileEntry {
        fid: String::from("FL-A1-99"),
        anchor: String::from("入口文件_备用流程锚点"),
    });
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let ds = gen_decls();
    let fs = gen_files();
    // —— 主路径：登记 → 四声明 → 逐条解引 → 归档 ——
    let mut m = MaintBoard::new();
    let mut i = 0usize;
    while i < fs.len() {
        m.register_file(fs[i].clone())?;
        i += 1;
    }
    if m.file_len() != 5 {
        return Err(XC_CONSERV_BREAK);
    }
    i = 0;
    while i < ds.len() {
        m.declare(ds[i].clone())?;
        i += 1;
    }
    if m.decl_len() != PATH_N {
        return Err(XC_PATH_SHORT);
    }
    if m.resolve()? != PATH_N as u32 {
        return Err(XC_NOT_ALL_PATH);
    }
    let rc = m.file_it()?;
    if rc.paths != 4 || !rc.all_flow || rc.files != 5 || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.file_it(), Err(XC_ALREADY_FILED)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 双跑对拍：同料重跑指纹一致（确定性）——
    let fp1 = rc.fingerprint;
    let fp2 = {
        let mut m2 = MaintBoard::new();
        let mut j = 0usize;
        while j < fs.len() {
            m2.register_file(fs[j].clone())?;
            j += 1;
        }
        j = 0;
        while j < ds.len() {
            m2.declare(ds[j].clone())?;
            j += 1;
        }
        m2.resolve()?;
        m2.fingerprint()
    };
    if fp1 != fp2 {
        return Err(XC_CONSERV_BREAK);
    }
    // —— 错误路径 1：通道重复 / 号重复 / 触发空 / 引用空 / 号越界 ——
    let mut m3 = MaintBoard::new();
    let mut d1 = ds.clone();
    d1[1].path = MaintPath::Errata;
    if m3.declare(d1[0].clone()).is_err() {
        return Err(XC_STATE_INVALID);
    }
    if !matches!(m3.declare(d1[1].clone()), Err(XC_DUP_PATH)) {
        return Err(XC_DUP_PATH);
    }
    let mut d2 = ds.clone();
    d2[1].pid = 1;
    if !matches!(m3.declare(d2[1].clone()), Err(XC_FILE_DUP)) {
        return Err(XC_FILE_DUP);
    }
    let mut d3 = ds.clone();
    d3[1].trigger = String::new();
    if !matches!(m3.declare(d3[1].clone()), Err(XC_TRIG_EMPTY)) {
        return Err(XC_TRIG_EMPTY);
    }
    let mut d4 = ds.clone();
    d4[1].flow_ref = String::new();
    if !matches!(m3.declare(d4[1].clone()), Err(XC_REF_EMPTY)) {
        return Err(XC_REF_EMPTY);
    }
    let mut d5 = ds.clone();
    d5[1].pid = 5;
    if !matches!(m3.declare(d5[1].clone()), Err(XC_PATH_OUT)) {
        return Err(XC_PATH_OUT);
    }
    // —— 错误路径 2：未登记解引 / 入口不足 4 / 声明不足 4 ——
    let mut m4 = MaintBoard::new();
    i = 0;
    while i < ds.len() {
        m4.declare(ds[i].clone())?;
        i += 1;
    }
    if !matches!(m4.resolve(), Err(XC_FILE_UNKNOWN)) {
        return Err(XC_FILE_UNKNOWN);
    }
    let mut m5 = MaintBoard::new();
    i = 0;
    while i < 3 {
        m5.register_file(fs[i].clone())?;
        m5.declare(ds[i].clone())?;
        i += 1;
    }
    if !matches!(m5.resolve(), Err(XC_PATH_SHORT)) {
        return Err(XC_PATH_SHORT);
    }
    let mut m6 = MaintBoard::new();
    i = 0;
    while i < 3 {
        m6.register_file(fs[i].clone())?;
        i += 1;
    }
    i = 0;
    while i < ds.len() {
        m6.declare(ds[i].clone())?;
        i += 1;
    }
    if !matches!(m6.resolve(), Err(XC_FILE_SHORT)) {
        return Err(XC_FILE_SHORT);
    }
    // —— 错误路径 3：入口 ID 重复 / 锚点空 / ID 空 ——
    let mut m7 = MaintBoard::new();
    if m7.register_file(FileEntry { fid: String::from("F"), anchor: String::from("a") }).is_err() {
        return Err(XC_STATE_INVALID);
    }
    if m7.register_file(FileEntry { fid: String::from("F"), anchor: String::from("b") }) != Err(XC_FILE_DUP) {
        return Err(XC_FILE_DUP);
    }
    if m7.register_file(FileEntry { fid: String::from("G"), anchor: String::new() }) != Err(XC_TRIG_EMPTY) {
        return Err(XC_TRIG_EMPTY);
    }
    if m7.register_file(FileEntry { fid: String::new(), anchor: String::from("c") }) != Err(XC_REF_EMPTY) {
        return Err(XC_REF_EMPTY);
    }
    // —— 查询 API：按号/按通道/按 ID 解引 + 直方图 + 未用守恒 ——
    match m.decl_of(2) {
        Some(d) if d.pid == 2 => {}
        _ => return Err(XC_PATH_SHORT),
    }
    if m.decl_of(9).is_some() {
        return Err(XC_FILE_DUP);
    }
    match m.path_of(MaintPath::Receipt) {
        Some(d) if d.flow_ref == "FL-A1-04" => {}
        _ => return Err(XC_NOT_ALL_PATH),
    }
    match m.file_of("FL-A1-02") {
        Some(f) if !f.anchor.is_empty() => {}
        _ => return Err(XC_FILE_UNKNOWN),
    }
    if m.file_of("NO-SUCH").is_some() {
        return Err(XC_FILE_UNKNOWN);
    }
    let oh = m.owner_histogram();
    if oh[0] + oh[1] != m.decl_len() as u32 || oh[0] != 2 || oh[1] != 2 {
        return Err(XC_CONSERV_BREAK);
    }
    let un = m.unused_files();
    if un.len() != 1 || un[0] != "FL-A1-99" {
        return Err(XC_CONSERV_BREAK); // 守恒：used 4 + unused 1 = 5
    }
    let tv = m.decl_table_view();
    if !tv.contains("1|chief|FL-A1-01") {
        return Err(XC_SUMMARY_EMPTY);
    }
    if m.stage().view() != "filed" {
        return Err(XC_STATE_INVALID);
    }
    Ok(0)
}
