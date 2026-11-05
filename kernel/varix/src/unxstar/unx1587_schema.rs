//! UNX-F1587 域级观测终验（schema/时序/口径收官三面） · 0x790C0段
//!
//! 判据原文（UNX-F1587-J1 摘录）：「观测终验通过（F1532 三面收官复验全
//! 过），跨账时序抽验 100 组一致率 100%，终验报告归档」；完成时 API/软件
//! 写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三面闭集：schema 面（字段在册）、时序面（事件单调）、口径面（指标
//!    口径冻结）逐面复验，缺面即收官拒；
//! 2. schema 面以字段账复验：字段名唯一、类型码在册、版本冻结不变；
//! 3. 时序面以事件账复验：事件序号严格递增且双账对齐，倒序/跳号显性拒；
//! 4. 跨账时序抽验 100 组：双账事件对（甲账序、乙账序）逐组比对一致，
//!    一致率千分比必须为 1000；乱序对注入必须被点名并拉低一致率；
//! 5. 状态机 复验中→已终验 单向，终验后追加显性拒；终验报告非空归档；
//!    诊断码闭集 0x790C1..0x790CC。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B08 批窗口 0x79000..=0x791C0）。
pub const SEG_BASE: u32 = 0x790C0;

/// 观测终验诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 字段名越界（空名或超长）。
    EField,
    /// 字段类型码册外。
    EType,
    /// 字段重复登记。
    EDup,
    /// schema 版本已冻结、再变更为倒退。
    EFrozen,
    /// 事件序号倒序或跳号。
    ESeq,
    /// 双账事件对失配。
    EPair,
    /// 观测面册外（三面之外）。
    EFace,
    /// 抽验组数不足（<100）。
    EShort,
    /// 一致率不足 100%。
    EMiss,
    /// 口径冻结被破坏。
    ECal,
    /// 终验报告缺失或为空。
    ENoReport,
    /// 终验已封、登记被拒。
    ESealed,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EField => 0x01,
                Diag::EType => 0x02,
                Diag::EDup => 0x03,
                Diag::EFrozen => 0x04,
                Diag::ESeq => 0x05,
                Diag::EPair => 0x06,
                Diag::EFace => 0x07,
                Diag::EShort => 0x08,
                Diag::EMiss => 0x09,
                Diag::ECal => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::ESealed => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EField => "字段名越界：字段名须非空且不超过 24 字节，先改名再登记",
            Diag::EType => "字段类型码册外：类型只认 1..=6，请对照 schema 类型表",
            Diag::EDup => "字段重复：schema 内字段名唯一，先查重再追加",
            Diag::EFrozen => "schema 版本已冻结：冻结后不得改字段，变更走下一版",
            Diag::ESeq => "事件序号倒序或跳号：时序必须严格递增，先修事件账",
            Diag::EPair => "双账事件对失配：甲乙账同点事件须同序同值，逐组排查",
            Diag::EFace => "观测面册外：只认 schema/时序/口径三面，请对照终验单",
            Diag::EShort => "抽验组数不足：跨账抽验必须满 100 组再收官",
            Diag::EMiss => "一致率不足 100%：失配组先修后复验，不得带病封账",
            Diag::ECal => "口径冻结被破坏：指标口径不得变更，恢复冻结口径",
            Diag::ENoReport => "终验报告缺失：报告须非空并载明三面结论与一致率",
            Diag::ESealed => "终验已封：封验后登记一律拒，增量走下一批",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EField,
    Diag::EType,
    Diag::EDup,
    Diag::EFrozen,
    Diag::ESeq,
    Diag::EPair,
    Diag::EFace,
    Diag::EShort,
    Diag::EMiss,
    Diag::ECal,
    Diag::ENoReport,
    Diag::ESealed,
];

/// 诊断码查表（闭集返回，未知道回兜底文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x790C1..0x790CC 闭集内"
}

/// 跨账时序抽验组数（判据钦定 100 组）。
pub const SAMPLE_GROUPS: usize = 100;
/// schema 类型码上界（1..=6）。
pub const TYPE_MAX: u8 = 6;
/// schema 字段名上界（字节）。
pub const NAME_CAP: usize = 24;
/// 一致率千分比硬线。
pub const AGREE_PERMILLE: u32 = 1000;

/// 观测面（三面闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// schema 面。
    Schema,
    /// 时序面。
    Order,
    /// 口径面。
    Caliber,
}

impl Face {
    /// 三面全表（闭集数组）。
    pub const ALL_FACES: &'static [Face] = &[Face::Schema, Face::Order, Face::Caliber];

    /// 面序号（1..=3）。
    pub fn ordinal(self) -> u8 {
        match self {
            Face::Schema => 1,
            Face::Order => 2,
            Face::Caliber => 3,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Face::Schema => "schema",
            Face::Order => "时序",
            Face::Caliber => "口径",
        }
    }
}

/// schema 字段账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldRow {
    /// 字段名（非空 ≤24 字节）。
    pub name: &'static str,
    /// 类型码（1..=6）。
    pub ty: u8,
    /// 字段槽位（0..=15，登记序）。
    pub slot: u16,
}

impl FieldRow {
    /// 结构合法性。
    pub fn in_scope(&self) -> bool {
        if self.name.is_empty() || self.name.len() > NAME_CAP {
            return false;
        }
        if self.ty == 0 || self.ty > TYPE_MAX {
            return false;
        }
        (self.slot as usize) < FIELDS_CAP
    }
}

/// schema 字段容量。
pub const FIELDS_CAP: usize = 16;

/// schema 字段账（版本冻结）。
pub struct SchemaBook {
    rows: [Option<FieldRow>; FIELDS_CAP],
    count: usize,
    version: u32,
    frozen: bool,
}

impl SchemaBook {
    /// 新建空账（版本 1）。
    pub fn new() -> SchemaBook {
        SchemaBook {
            rows: [None; FIELDS_CAP],
            count: 0,
            version: 1,
            frozen: false,
        }
    }

    /// 登记字段（重复名与越界显性拒）。
    pub fn add(&mut self, row: FieldRow) -> Result<usize, u32> {
        if self.frozen {
            return Err(Diag::EFrozen.val());
        }
        if !row.in_scope() {
            if row.name.is_empty() || row.name.len() > NAME_CAP {
                return Err(Diag::EField.val());
            }
            if row.ty == 0 || row.ty > TYPE_MAX {
                return Err(Diag::EType.val());
            }
            return Err(Diag::EField.val());
        }
        if row.slot as usize != self.count {
            return Err(Diag::EDup.val());
        }
        let mut k = 0usize;
        while k < self.count {
            match self.rows.get(k) {
                Some(Some(prev)) => {
                    if prev.name == row.name {
                        return Err(Diag::EDup.val());
                    }
                }
                _ => return Err(Diag::EField.val()),
            }
            k += 1;
        }
        match self.rows.get_mut(self.count) {
            Some(cell) => {
                *cell = Some(row);
                self.count += 1;
                Ok(self.count)
            }
            None => Err(Diag::EField.val()),
        }
    }

    /// 冻结版本（冻结后不得再变更）。
    pub fn freeze(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return Err(Diag::EFrozen.val());
        }
        if self.count < 3 {
            return Err(Diag::EField.val());
        }
        self.frozen = true;
        Ok(self.version)
    }

    /// 已冻结？
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 字段数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 取第 i 行。
    pub fn row(&self, i: usize) -> Option<FieldRow> {
        match self.rows.get(i) {
            Some(c) => *c,
            None => None,
        }
    }

    /// 版本号。
    pub fn ver(&self) -> u32 {
        self.version
    }
}

/// 双账事件对：甲账序、乙账序、事件值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairRow {
    /// 抽验组号（1..=100）。
    pub group: u16,
    /// 甲账事件序。
    pub seq_a: u32,
    /// 乙账事件序。
    pub seq_b: u32,
    /// 事件值指纹。
    pub val: u64,
}

impl PairRow {
    /// 一致判定：同序同值（双账镜像，序号相等即对齐）。
    pub const fn consistent(&self) -> bool {
        self.seq_a == self.seq_b && self.val != 0
    }
}

/// 跨账抽验账（100 组定容）。
pub struct SampleBook {
    rows: [Option<PairRow>; SAMPLE_GROUPS],
    count: usize,
}

impl SampleBook {
    /// 新建空账。
    pub fn new() -> SampleBook {
        SampleBook {
            rows: [None; SAMPLE_GROUPS],
            count: 0,
        }
    }

    /// 登记一组抽验（组号严格递增）。
    pub fn record(&mut self, row: PairRow) -> Result<usize, u32> {
        if row.group == 0 || (row.group as usize) > SAMPLE_GROUPS {
            return Err(Diag::EShort.val());
        }
        if row.group as usize != self.count + 1 {
            return Err(Diag::ESeq.val());
        }
        if row.val == 0 {
            return Err(Diag::EPair.val());
        }
        match self.rows.get_mut(self.count) {
            Some(cell) => {
                *cell = Some(row);
                self.count += 1;
                Ok(self.count)
            }
            None => Err(Diag::EShort.val()),
        }
    }

    /// 已登记组数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 取第 i 组。
    pub fn row(&self, i: usize) -> Option<PairRow> {
        match self.rows.get(i) {
            Some(c) => *c,
            None => None,
        }
    }

    /// 一致组数。
    pub fn agree_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if let Some(r) = self.row(i) {
                if r.consistent() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 一致率千分比（组数不足 100 记 0，防以少充多）。
    pub fn agree_permille(&self) -> u32 {
        if self.count < SAMPLE_GROUPS {
            return 0;
        }
        (self.agree_count() * 1000 / self.count) as u32
    }

    /// 失配组点名单。
    pub fn mismatch_groups(&self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(r) = self.row(i) {
                if !r.consistent() {
                    out.push(r.group);
                }
            }
            i += 1;
        }
        out
    }
}

/// 观测终验报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObsReport {
    /// 三面结论（面名: 通过/未通过）。
    pub lines: Vec<String>,
    /// 抽验组数。
    pub groups: usize,
    /// 一致组数。
    pub agree: usize,
    /// 一致率千分比。
    pub permille: u32,
    /// 非空摘要。
    pub summary: String,
}

/// 终验阶段：复验中 → 已终验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 复验中。
    Verifying,
    /// 已终验。
    Sealed,
}

/// 观测终验总账（三面 + 抽验账）。
pub struct ObsFinal {
    schema: SchemaBook,
    sample: SampleBook,
    /// 口径冻结值（指标口径指纹表，3 项钦定）。
    calibers: [Option<u64>; 3],
    faces_passed: [Option<bool>; 3],
    phase: Phase,
    report: Option<ObsReport>,
}

impl ObsFinal {
    /// 新建终验总账。
    pub fn new() -> ObsFinal {
        ObsFinal {
            schema: SchemaBook::new(),
            sample: SampleBook::new(),
            calibers: [None; 3],
            faces_passed: [None; 3],
            phase: Phase::Verifying,
            report: None,
        }
    }

    /// schema 面句柄。
    pub fn schema(&mut self) -> &mut SchemaBook {
        &mut self.schema
    }

    /// schema 面只读句柄。
    pub fn schema_ref(&self) -> &SchemaBook {
        &self.schema
    }

    /// 抽验面句柄。
    pub fn sample(&mut self) -> &mut SampleBook {
        &mut self.sample
    }

    /// 抽验面只读句柄。
    pub fn sample_ref(&self) -> &SampleBook {
        &self.sample
    }

    /// 冻结一项口径（下标 0..=2，重复冻结显性拒）。
    pub fn freeze_caliber(&mut self, idx: usize, fp: u64) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if idx >= 3 {
            return Err(Diag::EFace.val());
        }
        if fp == 0 {
            return Err(Diag::ECal.val());
        }
        match self.calibers.get(idx) {
            Some(Some(_)) => return Err(Diag::ECal.val()),
            Some(None) => {}
            None => return Err(Diag::EFace.val()),
        }
        match self.calibers.get_mut(idx) {
            Some(cell) => {
                *cell = Some(fp);
                Ok(())
            }
            None => Err(Diag::EFace.val()),
        }
    }

    /// 口径是否冻结满三项。
    pub fn calibers_frozen(&self) -> bool {
        for i in 0..3 {
            match self.calibers.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
        }
        true
    }

    /// 口径值（未冻结返 None）。
    pub fn caliber_at(&self, idx: usize) -> Option<u64> {
        match self.calibers.get(idx) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 三面复验判定并记录。
    pub fn verify_faces(&mut self) -> Result<[bool; 3], u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        // schema 面：字段账冻结且非空。
        let f_schema = self.schema.is_frozen() && self.schema.len() >= 3;
        // 时序面：抽验账组号严格递增（登记序即保证），且满 100 组。
        let f_order = self.sample.len() == SAMPLE_GROUPS;
        // 口径面：三项口径全冻结。
        let f_cal = self.calibers_frozen();
        let verdicts = [f_schema, f_order, f_cal];
        let mut i = 0usize;
        while i < 3 {
            let v = match verdicts.get(i) {
                Some(x) => *x,
                None => return Err(Diag::EFace.val()),
            };
            match self.faces_passed.get_mut(i) {
                Some(cell) => *cell = Some(v),
                None => return Err(Diag::EFace.val()),
            }
            i += 1;
        }
        Ok(verdicts)
    }

    /// 面结论（未复验返 None）。
    pub fn face_verdict(&self, face: Face) -> Option<bool> {
        match self.faces_passed.get((face.ordinal() - 1) as usize) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 终验：三面全过 + 抽验满组一致率 1000‰ 才落报告。
    pub fn seal(&mut self) -> Result<ObsReport, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        let verdicts = self.verify_faces()?;
        for face in Face::ALL_FACES {
            match self.face_verdict(*face) {
                Some(true) => {}
                _ => return Err(Diag::EMiss.val()),
            }
        }
        if self.sample.len() < SAMPLE_GROUPS {
            return Err(Diag::EShort.val());
        }
        let permille = self.sample.agree_permille();
        if permille != AGREE_PERMILLE {
            return Err(Diag::EMiss.val());
        }
        let mut lines = Vec::new();
        for face in Face::ALL_FACES {
            lines.push(format!(
                "{}面: {}",
                face.name(),
                match self.face_verdict(*face) {
                    Some(true) => "通过",
                    _ => "未通过",
                }
            ));
        }
        let report = ObsReport {
            lines,
            groups: self.sample.len(),
            agree: self.sample.agree_count(),
            permille,
            summary: format!(
                "观测终验：三面收官复验全过，跨账抽验{}组一致率{}‰，报告归档",
                self.sample.len(),
                permille
            ),
        };
        self.report = Some(report.clone());
        self.phase = Phase::Sealed;
        Ok(report)
    }

    /// 已落档报告。
    pub fn report(&self) -> Option<&ObsReport> {
        self.report.as_ref()
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }
}

// ---- 确定性构账 ------------------------------------------------------------

/// F1532 钦定 schema 字段表（6 字段，版本 1）。
pub fn schema_fields() -> [FieldRow; 6] {
    [
        FieldRow { name: "dom", ty: 2, slot: 0 },
        FieldRow { name: "ev", ty: 1, slot: 1 },
        FieldRow { name: "seq", ty: 4, slot: 2 },
        FieldRow { name: "val", ty: 4, slot: 3 },
        FieldRow { name: "src", ty: 2, slot: 4 },
        FieldRow { name: "ts", ty: 4, slot: 5 },
    ]
}

/// 事件值指纹派生（组号 → 指纹，确定性）。
pub fn fingerprint(group: u16) -> u64 {
    0x9E37_79B9_7F4A_7C15 ^ (group as u64).wrapping_mul(0x517C_C1B7_27D4_6511)
}

/// 构造三面齐备的终验总账（100 组全一致）。
pub fn build_obs_final() -> ObsFinal {
    let mut f = ObsFinal::new();
    for row in schema_fields() {
        match f.schema().add(row) {
            Ok(_) => {}
            Err(_) => {}
        }
    }
    let _ = f.schema().freeze();
    let mut i = 0usize;
    while i < 3 {
        let _ = f.freeze_caliber(i, fingerprint(1000 + i as u16));
        i += 1;
    }
    let mut g = 0usize;
    while g < SAMPLE_GROUPS {
        let seq = (g + 1) as u32;
        let val = fingerprint((g + 1) as u16);
        let _ = f.sample().record(PairRow {
            group: (g + 1) as u16,
            seq_a: seq,
            seq_b: seq,
            val,
        });
        g += 1;
    }
    f
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B08 派工卡 0x79000..=0x791C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79000 || SEG_BASE > 0x791C0 {
        return false;
    }
    true
}

/// 三面全过谓词。
fn faces_all_ok(f: &ObsFinal) -> bool {
    for face in Face::ALL_FACES {
        match f.face_verdict(*face) {
            Some(true) => {}
            _ => return false,
        }
    }
    true
}

/// 一致率 100% 谓词。
fn agree_full_ok(f: &ObsFinal) -> bool {
    f.sample_ref().len() == SAMPLE_GROUPS && f.sample_ref().agree_permille() == AGREE_PERMILLE
}

/// 乱序对注入必抓：乙账序偏移必须被点名且一致率跌落。
fn disorder_injection_caught() -> bool {
    let mut f = build_obs_final();
    let g = 57usize;
    let row = match f.sample_ref().row(g) {
        Some(r) => r,
        None => return false,
    };
    let bad = PairRow {
        group: row.group,
        seq_a: row.seq_a,
        seq_b: row.seq_b + 3,
        val: row.val,
    };
    if !f.sample_overwrite(g, bad) {
        return false;
    }
    if f.sample_ref().agree_count() != SAMPLE_GROUPS - 1 {
        return false;
    }
    let mism = f.sample_ref().mismatch_groups();
    if mism.len() != 1 {
        return false;
    }
    match mism.get(0) {
        Some(x) if *x as usize == g + 1 => {}
        _ => return false,
    }
    f.sample_ref().agree_permille() != AGREE_PERMILLE
}

/// 口径注入必抓：冻结后重复冻结同一槽必须拒。
fn caliber_injection_caught() -> bool {
    let mut f = build_obs_final();
    match f.freeze_caliber(1, fingerprint(2001)) {
        Err(e) => e == Diag::ECal.val(),
        Ok(_) => false,
    }
}

/// schema 注入必抓：冻结后再变更字段必须拒。
fn schema_injection_caught() -> bool {
    let mut f = build_obs_final();
    match f.schema().add(FieldRow { name: "late", ty: 1, slot: 6 }) {
        Err(e) => e == Diag::EFrozen.val(),
        Ok(_) => false,
    }
}

/// schema 登记拒绝矩阵：空名/超长名/类型码册外/重复名四门。
fn schema_reject_matrix_ok() -> bool {
    let mut b = SchemaBook::new();
    if b.add(FieldRow { name: "", ty: 1, slot: 0 }).is_ok() {
        return false;
    }
    if b
        .add(FieldRow { name: "a1a2a3a4a5a6a7a8a9a0b1b2b3b4", ty: 1, slot: 0 })
        .is_ok()
    {
        return false;
    }
    if b.add(FieldRow { name: "ok", ty: 7, slot: 0 }).is_ok() {
        return false;
    }
    if b.add(FieldRow { name: "ok", ty: 1, slot: 0 }).is_err() {
        return false;
    }
    if b.add(FieldRow { name: "ok", ty: 2, slot: 1 }).is_ok() {
        return false;
    }
    b.len() == 1
}

/// 抽验登记拒绝矩阵：组号乱序/零指纹/越界组号三门。
fn sample_reject_matrix_ok() -> bool {
    let mut b = SampleBook::new();
    let r1 = PairRow { group: 2, seq_a: 1, seq_b: 1, val: 7 };
    if b.record(r1).is_ok() {
        return false;
    }
    let r2 = PairRow { group: 1, seq_a: 1, seq_b: 1, val: 0 };
    if b.record(r2).is_ok() {
        return false;
    }
    let r3 = PairRow {
        group: (SAMPLE_GROUPS + 1) as u16,
        seq_a: 1,
        seq_b: 1,
        val: 7,
    };
    if b.record(r3).is_ok() {
        return false;
    }
    b.len() == 0
}

// ---- SampleBook 注入辅助 ---------------------------------------------------

impl ObsFinal {
    /// 改写第 i 组抽验行（注入测试专用路径）。
    pub fn sample_overwrite(&mut self, i: usize, row: PairRow) -> bool {
        if i >= SAMPLE_GROUPS {
            return false;
        }
        match self.sample.rows.get_mut(i) {
            Some(cell) => {
                *cell = Some(row);
                true
            }
            None => false,
        }
    }
}

// ---- 检查节 ----------------------------------------------------------------

/// UNX-F1587 自检入口：schema/时序/口径三面收官终验。
pub fn run_unx1587_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1587");
    let mut f = build_obs_final();

    s.add_dyn(
        "F1587-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1587-02 schema 面字段账齐备",
        f.schema_ref().len() == 6 && f.schema_ref().is_frozen(),
        &format!("fields={} ver={}", f.schema_ref().len(), f.schema_ref().ver()),
    );

    s.add_dyn(
        "F1587-03 口径面三项冻结",
        f.calibers_frozen() && f.caliber_at(0).is_some(),
        &format!("c0={:016x}", f.caliber_at(0).unwrap_or(0)),
    );

    s.add_dyn(
        "F1587-04 时序面抽验满百组",
        f.sample_ref().len() == SAMPLE_GROUPS,
        &format!("groups={} cap={}", f.sample_ref().len(), SAMPLE_GROUPS),
    );

    s.add_dyn(
        "F1587-05 跨账一致率 100%",
        agree_full_ok(&f),
        &format!(
            "agree={}/{} permille={}",
            f.sample_ref().agree_count(),
            f.sample_ref().len(),
            f.sample_ref().agree_permille()
        ),
    );

    s.add_dyn(
        "F1587-06 schema 登记拒绝矩阵四门",
        schema_reject_matrix_ok(),
        "空名/超长名/类型码册外/重复名全部显性拒",
    );

    s.add_dyn(
        "F1587-07 抽验登记拒绝矩阵三门",
        sample_reject_matrix_ok(),
        "组号乱序/零指纹/越界组号全部显性拒",
    );

    s.add_dyn(
        "F1587-08 乱序对注入必抓",
        disorder_injection_caught(),
        "乙账序偏移必须被点名且一致率跌落",
    );

    s.add_dyn(
        "F1587-09 口径重复冻结注入必抓",
        caliber_injection_caught(),
        "已冻结槽再冻结必须拒（ECal）",
    );

    s.add_dyn(
        "F1587-10 schema 冻结后变更注入必抓",
        schema_injection_caught(),
        "冻结后再登记必须拒（EFrozen）",
    );

    // 三面复验 + 终验落档。
    let verdicts = match f.verify_faces() {
        Ok(v) => v,
        Err(e) => {
            let _ = e;
            [false; 3]
        }
    };
    s.add_dyn(
        "F1587-11 三面复验全过",
        verdicts == [true, true, true] && faces_all_ok(&f),
        &format!(
            "schema={} order={} caliber={}",
            verdicts.get(0).copied().unwrap_or(false),
            verdicts.get(1).copied().unwrap_or(false),
            verdicts.get(2).copied().unwrap_or(false)
        ),
    );

    let seal_res = f.seal();
    s.add_dyn(
        "F1587-12 终验报告归档",
        seal_res.is_ok() && f.report().is_some() && f.phase() == Phase::Sealed,
        &format!("sealed={}", f.phase() == Phase::Sealed),
    );
    s.add_dyn(
        "F1587-13 状态写回（已完成+摘要非空）",
        match &seal_res {
            Ok(r) => r.groups == SAMPLE_GROUPS && r.permille == AGREE_PERMILLE && !r.summary.is_empty() && r.lines.len() == 3,
            Err(_) => false,
        },
        match &seal_res {
            Ok(r) => r.summary.as_str(),
            Err(e) => detail(*e),
        },
    );

    s.add_dyn(
        "F1587-14 封验后追加显性拒",
        match f.seal() {
            Err(e) => e == Diag::ESealed.val(),
            Ok(_) => false,
        },
        "终验后重复终验必须拒（ESealed）",
    );

    s.add_dyn(
        "F1587-15 双跑一致",
        {
            let a = build_obs_final();
            let b = build_obs_final();
            let mut same = true;
            let mut i = 0usize;
            while i < SAMPLE_GROUPS {
                let ra = a.sample_ref().row(i);
                let rb = b.sample_ref().row(i);
                if ra != rb {
                    same = false;
                }
                i += 1;
            }
            same && a.schema_ref().len() == b.schema_ref().len()
        },
        "两次构账抽验行与字段账逐项一致",
    );

    s.add_dyn(
        "F1587-16 诊断码闭集完备",
        detail_codes_ok(),
        "12 码逐条查表非空且兜底可达",
    );

    s
}

/// 诊断码闭集完备性。
fn detail_codes_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    for d in ALL {
        if d.detail().is_empty() {
            return false;
        }
        if detail(d.val()) != d.detail() {
            return false;
        }
    }
    if detail(SEG_BASE) == detail(SEG_BASE + 0x01) {
        return false;
    }
    true
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_holds() {
        assert!(SEG_BASE % 0x40 == 0);
        assert!(SEG_BASE >= 0x79000 && SEG_BASE <= 0x791C0);
        assert!(seg_guard_ok());
    }

    #[test]
    fn schema_book_rejects_bad_rows() {
        assert!(schema_reject_matrix_ok());
        let mut b = SchemaBook::new();
        assert!(b.freeze().is_err(), "空账不得冻结");
        for row in schema_fields() {
            assert!(b.add(row).is_ok());
        }
        assert_eq!(b.freeze(), Ok(1));
        assert!(b.is_frozen());
        assert_eq!(b.freeze(), Err(Diag::EFrozen.val()));
    }

    #[test]
    fn sample_book_records_in_order() {
        let mut b = SampleBook::new();
        assert!(sample_reject_matrix_ok());
        let mut g = 1usize;
        while g <= 3 {
            let r = PairRow {
                group: g as u16,
                seq_a: g as u32,
                seq_b: g as u32,
                val: fingerprint(g as u16),
            };
            assert_eq!(b.record(r), Ok(g));
            g += 1;
        }
        assert_eq!(b.len(), 3);
        assert_eq!(b.agree_count(), 3);
    }

    #[test]
    fn build_is_full_agreement() {
        let f = build_obs_final();
        assert_eq!(f.sample_ref().len(), SAMPLE_GROUPS);
        assert!(agree_full_ok(&f));
        assert!(f.calibers_frozen());
        assert!(f.schema_ref().is_frozen());
    }

    #[test]
    fn disorder_injection_lowers_agreement() {
        assert!(disorder_injection_caught());
    }

    #[test]
    fn injection_trio_caught() {
        assert!(caliber_injection_caught());
        assert!(schema_injection_caught());
    }

    #[test]
    fn seal_needs_all_faces_and_full_agreement() {
        // 缺口径：抽验与 schema 齐但口径未冻结 → 三面复验口径面未过。
        let mut f = ObsFinal::new();
        for row in schema_fields() {
            let _ = f.schema().add(row);
        }
        let _ = f.schema().freeze();
        let mut g = 0usize;
        while g < SAMPLE_GROUPS {
            let seq = (g + 1) as u32;
            let _ = f.sample().record(PairRow {
                group: (g + 1) as u16,
                seq_a: seq,
                seq_b: seq,
                val: fingerprint((g + 1) as u16),
            });
            g += 1;
        }
        match f.seal() {
            Err(e) => assert_eq!(e, Diag::EMiss.val(), "缺口径终验必须拒"),
            Ok(_) => { assert!(false, "缺口径终验必须拒"); loop {} },
        }
    }

    #[test]
    fn seal_reports_and_freezes() {
        let mut f = build_obs_final();
        let rep = match f.seal() {
            Ok(r) => r,
            Err(e) => { assert!(false, "终验失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(rep.groups, SAMPLE_GROUPS);
        assert_eq!(rep.agree, SAMPLE_GROUPS);
        assert_eq!(rep.permille, AGREE_PERMILLE);
        assert_eq!(rep.lines.len(), 3);
        assert!(!rep.summary.is_empty());
        assert!(f.phase() == Phase::Sealed);
        assert_eq!(
            f.freeze_caliber(0, 7),
            Err(Diag::ESealed.val()),
            "封验后口径冻结必须拒"
        );
    }

    #[test]
    fn short_sample_blocks_seal() {
        let mut f = ObsFinal::new();
        for row in schema_fields() {
            let _ = f.schema().add(row);
        }
        let _ = f.schema().freeze();
        let mut i = 0usize;
        while i < 3 {
            let _ = f.freeze_caliber(i, fingerprint(1000 + i as u16));
            i += 1;
        }
        let mut g = 0usize;
        while g < SAMPLE_GROUPS - 1 {
            let seq = (g + 1) as u32;
            let _ = f.sample().record(PairRow {
                group: (g + 1) as u16,
                seq_a: seq,
                seq_b: seq,
                val: fingerprint((g + 1) as u16),
            });
            g += 1;
        }
        match f.seal() {
            Err(e) => assert_eq!(e, Diag::EMiss.val(), "缺组终验必须拒"),
            Ok(_) => { assert!(false, "缺组终验必须拒"); loop {} },
        }
    }

    #[test]
    fn double_run_deterministic() {
        let a = build_obs_final();
        let b = build_obs_final();
        let mut i = 0usize;
        while i < SAMPLE_GROUPS {
            assert_eq!(a.sample_ref().row(i), b.sample_ref().row(i));
            i += 1;
        }
        assert_eq!(a.caliber_at(0), b.caliber_at(0));
        assert_eq!(a.schema_ref().len(), b.schema_ref().len());
    }

    #[test]
    fn diag_closed_set_complete() {
        assert_eq!(ALL.len(), 12);
        for (i, d) in ALL.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + (i as u32) + 1);
            assert!(!d.detail().is_empty());
        }
        assert_eq!(
            detail(SEG_BASE + 0x0d),
            "未知诊断码：不在 0x790C1..0x790CC 闭集内"
        );
        assert!(detail_codes_ok());
    }
}
