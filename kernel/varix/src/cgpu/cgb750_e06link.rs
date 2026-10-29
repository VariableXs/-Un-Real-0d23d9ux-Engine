//! cgb750_e06link —— CGPU-F0750 画像与 E06 场景联动。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0750 画像与 E06 场景联动  <SEG 0x4c00>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0750；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. E06 场景 = 已归档的历史场景账（只读遗产面）；本模块把
//!      新画像体系挂接 E06 遗产场景：挂接规则 = 遗产场景类别码
//!      到画像需求掩码的静态映射表（写死），映射表外类别红码；
//!   2. 挂接方向单向：画像 → E06 场景（遗产面禁止反向写——E06
//!      是只读账，任何写尝试都是红线）；
//!   3. 挂接状态机三态：Detached→Attached→Verified，Verified 后
//!      断接红码（封账级）；断接仅限 Attached 态；
//!   4. 挂接账守恒：Σ挂接记录 = Σattach − Σdetach，逐条可复算；
//!   5. E06 场景类别码闭集 8 类（写死表），映射到需求掩码
//      （低 4 位场景大类位）；画像掩码 ⊇ 需求掩码才可挂；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4c00 段为本模块独占（SEG_BASE=0x4c00），判别式
//!   SEG_BASE+1..+N；与 0x4be0/0x4c20 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const EL_VERSION: &str = "cgb750_e06link-v1";

/// 诊断段基址（0x4c00 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4C00;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4c01 E06 类别码越界（0..=7 之外）。
pub const XC_BAD_E06KIND: u16 = SEG_BASE + 1;
/// 0x4c02 遗产场景不存在（账内无此场景号）。
pub const XC_SCENE_ABSENT: u16 = SEG_BASE + 2;
/// 0x4c03 画像掩码不足（画像掩码 ⊉ 需求掩码）。
pub const XC_MASK_INSUFF: u16 = SEG_BASE + 3;
/// 0x4c04 遗产面写尝试（E06 只读红线）。
pub const XC_E06_READONLY: u16 = SEG_BASE + 4;
/// 0x4c05 挂接状态机非法边。
pub const XC_BAD_EDGE: u16 = SEG_BASE + 5;
/// 0x4c06 Verified 后断接（封账级不可变被破坏）。
pub const XC_VERIFIED_DETACH: u16 = SEG_BASE + 6;
/// 0x4c07 重复挂接（同场景同画像已挂）。
pub const XC_DOUBLE_ATTACH: u16 = SEG_BASE + 7;
/// 0x4c08 挂接账不守恒。
pub const XC_LEDGER_UNBALANCED: u16 = SEG_BASE + 8;
/// 0x4c09 场景容量满（32 槽遗产账已尽）。
pub const XC_SCENE_FULL: u16 = SEG_BASE + 9;

/// E06 遗产场景容量（写死：32）。
pub const E06_CAP: usize = 32;

/// E06 场景类别码 → 画像需求掩码映射表（写死：册内 §E06 类别表；
/// 低 4 位 = 场景大类位 bit0 渲染 bit1 计算 bit2 媒体 bit3 空闲）。
pub const E06_KIND_MASK: [u8; 8] = [
    0b0001, // 0 纯渲染遗产场景
    0b0010, // 1 纯计算
    0b0100, // 2 纯媒体
    0b1000, // 3 纯空闲
    0b0011, // 4 渲染+计算混合
    0b0110, // 5 计算+媒体混合
    0b0101, // 6 渲染+媒体混合
    0b0111, // 7 渲染+计算+媒体三混
];

/// E06 遗产场景（只读属性集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct E06Scene {
    /// 遗产场景号。
    pub sid: u16,
    /// 类别码（0..=7）。
    pub kind: u8,
    /// 遗产权重（0..=100）。
    pub legacy_weight: u8,
}

/// 挂接状态三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachState {
    /// 未挂。
    Detached,
    /// 已挂未验。
    Attached,
    /// 已验（终态）。
    Verified,
}

/// 合法边：Detached→Attached→Verified；Attached→Detached（断接）。
pub fn legal_edges(s: AttachState) -> &'static [AttachState] {
    match s {
        AttachState::Detached => &[AttachState::Attached],
        AttachState::Attached => &[AttachState::Verified, AttachState::Detached],
        AttachState::Verified => &[],
    }
}

fn step(cur: AttachState, next: AttachState) -> Result<AttachState, u16> {
    if legal_edges(cur).iter().any(|n| *n == next) {
        Ok(next)
    } else {
        Err(XC_BAD_EDGE)
    }
}

/// 挂接记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachRecord {
    /// 画像号。
    pub pid: u16,
    /// 遗产场景号。
    pub sid: u16,
    /// 记录类型：0=attach 1=detach 2=verify。
    pub kind: u8,
}

/// E06 遗产账（只读面：登记后不可改属性，仅可整体查询）。
pub struct E06Ledger {
    scenes: [Option<E06Scene>; E06_CAP],
    /// 写尝试拦截计数（红线审计面）。
    pub write_blocked: u32,
}

impl E06Ledger {
    /// 空账。
    pub fn new() -> Self {
        E06Ledger {
            scenes: [None; E06_CAP],
            write_blocked: 0,
        }
    }

    /// 登记遗产场景（仅登记期可写，号重复/越界红码）。
    pub fn admit(&mut self, s: E06Scene) -> Result<(), u16> {
        if (s.sid as usize) >= E06_CAP {
            return Err(XC_SCENE_FULL);
        }
        if s.kind > 7 {
            return Err(XC_BAD_E06KIND);
        }
        if self.scenes[s.sid as usize].is_some() {
            return Err(XC_SCENE_FULL);
        }
        if s.legacy_weight > 100 {
            return Err(XC_BAD_E06KIND);
        }
        self.scenes[s.sid as usize] = Some(s);
        Ok(())
    }

    /// 查询。
    pub fn get(&self, sid: u16) -> Result<E06Scene, u16> {
        self.scenes
            .get(sid as usize)
            .and_then(|o| o.as_ref())
            .copied()
            .ok_or(XC_SCENE_ABSENT)
    }

    /// 遗产属性改写接口——永远红码并计数（只读红线演示面）。
    pub fn rewrite(&mut self, _sid: u16, _kind: u8) -> Result<(), u16> {
        self.write_blocked += 1;
        Err(XC_E06_READONLY)
    }

    /// 需求掩码查询（类别映射表）。
    pub fn required_mask(&self, sid: u16) -> Result<u8, u16> {
        let s = self.get(sid)?;
        E06_KIND_MASK
            .get(s.kind as usize)
            .copied()
            .ok_or(XC_BAD_E06KIND)
    }
}

/// 挂接器：画像掩码登记 + 挂接状态机 + 挂接账。
pub struct E06Linker {
    /// 画像掩码表（pid → 场景大类位图）。
    pub profile_masks: Vec<(u16, u8)>,
    e06: E06Ledger,
    /// 挂接状态账：键 (pid, sid) → 状态。
    states: Vec<((u16, u16), AttachState)>,
    ledger: Vec<AttachRecord>,
    /// attach 计数。
    pub attaches: u32,
    /// detach 计数。
    pub detaches: u32,
}

impl E06Linker {
    /// 空挂接器。
    pub fn new(e06: E06Ledger) -> Self {
        E06Linker {
            profile_masks: Vec::new(),
            e06,
            states: Vec::new(),
            ledger: Vec::new(),
            attaches: 0,
            detaches: 0,
        }
    }

    /// 登记画像掩码（重复登记红码）。
    pub fn admit_profile(&mut self, pid: u16, mask: u8) -> Result<(), u16> {
        if self.profile_masks.iter().any(|(p, _)| *p == pid) {
            return Err(XC_DOUBLE_ATTACH);
        }
        if mask > 0x0F {
            return Err(XC_MASK_INSUFF);
        }
        self.profile_masks.push((pid, mask));
        Ok(())
    }

    fn mask_of(&self, pid: u16) -> Result<u8, u16> {
        self.profile_masks
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|(_, m)| *m)
            .ok_or(XC_SCENE_ABSENT)
    }

    /// 挂接主路径：掩码覆盖核 → 状态机 Detached→Attached → 落账。
    pub fn attach(&mut self, pid: u16, sid: u16) -> Result<AttachState, u16> {
        // 遗产面存在性 + 需求掩码。
        let need = self.e06.required_mask(sid)?;
        let have = self.mask_of(pid)?;
        if (have & need) != need {
            return Err(XC_MASK_INSUFF);
        }
        // 状态机。
        let cur = self
            .states
            .iter()
            .find(|(k, _)| *k == (pid, sid))
            .map(|(_, s)| *s)
            .unwrap_or(AttachState::Detached);
        if cur != AttachState::Detached {
            return Err(XC_DOUBLE_ATTACH);
        }
        let next = step(cur, AttachState::Attached)?;
        if let Some(slot) = self.states.iter_mut().find(|(k, _)| *k == (pid, sid)) {
            slot.1 = next;
        } else {
            self.states.push(((pid, sid), next));
        }
        self.ledger.push(AttachRecord { pid, sid, kind: 0 });
        self.attaches += 1;
        Ok(next)
    }

    /// 验收：Attached→Verified（终态）。
    pub fn verify(&mut self, pid: u16, sid: u16) -> Result<AttachState, u16> {
        let slot = self
            .states
            .iter_mut()
            .find(|(k, _)| *k == (pid, sid))
            .ok_or(XC_BAD_EDGE)?;
        let next = step(slot.1, AttachState::Verified)?;
        slot.1 = next;
        self.ledger.push(AttachRecord { pid, sid, kind: 2 });
        Ok(next)
    }

    /// 断接：仅 Attached 态允许；Verified 断接红码。
    pub fn detach(&mut self, pid: u16, sid: u16) -> Result<AttachState, u16> {
        let slot = self
            .states
            .iter_mut()
            .find(|(k, _)| *k == (pid, sid))
            .ok_or(XC_BAD_EDGE)?;
        if slot.1 == AttachState::Verified {
            return Err(XC_VERIFIED_DETACH);
        }
        let next = step(slot.1, AttachState::Detached)?;
        slot.1 = next;
        self.ledger.push(AttachRecord { pid, sid, kind: 1 });
        self.detaches += 1;
        Ok(next)
    }

    /// 挂接账守恒：账内 attach 条数 = attaches 计数；detach 同理。
    pub fn conserved(&self) -> Result<(), u16> {
        let a = self.ledger.iter().filter(|r| r.kind == 0).count() as u32;
        let d = self.ledger.iter().filter(|r| r.kind == 1).count() as u32;
        let v = self.ledger.iter().filter(|r| r.kind == 2).count() as u32;
        if a != self.attaches || d != self.detaches {
            return Err(XC_LEDGER_UNBALANCED);
        }
        // 在账状态数 ≤ attach 总数（每键最多一条在册）。
        if self.states.len() as u32 > a {
            return Err(XC_LEDGER_UNBALANCED);
        }
        let _ = v;
        Ok(())
    }

    /// 在册挂接数（Attached 或 Verified 态）。
    pub fn live_links(&self) -> usize {
        self.states
            .iter()
            .filter(|(_, s)| *s != AttachState::Detached)
            .count()
    }

    /// 遗产面只读红旗计数透出。
    pub fn e06_write_blocked(&self) -> u32 {
        self.e06.write_blocked
    }
}

/// 自例程：遗产账登记 → 画像掩码登记 → 挂接/验收/断接 → 守恒 →
/// 错误路径（掩码不足/遗产写/重复挂接/Verified 断接/类别越界）。
pub fn drill() -> Result<u32, u16> {
    let mut e06 = E06Ledger::new();
    e06.admit(E06Scene { sid: 0, kind: 4, legacy_weight: 80 })?;
    e06.admit(E06Scene { sid: 1, kind: 2, legacy_weight: 50 })?;
    e06.admit(E06Scene { sid: 2, kind: 7, legacy_weight: 100 })?;

    // 错误路径 1：遗产面写红线。
    if e06.rewrite(0, 1) != Err(XC_E06_READONLY) {
        return Err(XC_E06_READONLY);
    }
    if e06.write_blocked != 1 {
        return Err(XC_E06_READONLY);
    }

    let mut lk = E06Linker::new(e06);
    lk.admit_profile(0, 0b0111)?; // 渲染+计算+媒体
    lk.admit_profile(1, 0b0100)?; // 仅媒体
    // 重复登记红码。
    if lk.admit_profile(0, 0b0001) != Err(XC_DOUBLE_ATTACH) {
        return Err(XC_DOUBLE_ATTACH);
    }

    // 主路径：pid0 挂 sid0（需求 0b0011 ⊆ 0b0111）。
    let st = lk.attach(0, 0)?;
    if st != AttachState::Attached {
        return Err(XC_BAD_EDGE);
    }
    lk.verify(0, 0)?;
    // Verified 后断接红码。
    if lk.detach(0, 0) != Err(XC_VERIFIED_DETACH) {
        return Err(XC_VERIFIED_DETACH);
    }
    // 重复挂接红码。
    if lk.attach(0, 0) != Err(XC_DOUBLE_ATTACH) {
        return Err(XC_DOUBLE_ATTACH);
    }
    // pid0 挂 sid2（需求 0b0111 全覆盖）→ 挂接 → 断接 → 再挂。
    lk.attach(0, 2)?;
    lk.detach(0, 2)?;
    lk.attach(0, 2)?;
    // pid1 挂 sid0：需求 0b0011 ⊄ 0b0100 → 红码。
    if lk.attach(1, 0) != Err(XC_MASK_INSUFF) {
        return Err(XC_MASK_INSUFF);
    }
    // pid1 挂 sid1（需求 0b0100）→ 成功。
    lk.attach(1, 1)?;

    lk.conserved()?;
    // live：pid0-sid0(V) + pid0-sid2(A) + pid1-sid1(A) = 3。
    if lk.live_links() != 3 {
        return Err(XC_LEDGER_UNBALANCED);
    }
    if lk.attaches != 4 || lk.detaches != 1 {
        return Err(XC_LEDGER_UNBALANCED);
    }

    // 错误路径：类别码越界登记。
    let mut e06b = E06Ledger::new();
    if e06b.admit(E06Scene { sid: 0, kind: 8, legacy_weight: 1 }) != Err(XC_BAD_E06KIND) {
        return Err(XC_BAD_E06KIND);
    }
    // 错误路径：不存在场景挂接。
    if lk.attach(0, 9) != Err(XC_SCENE_ABSENT) {
        return Err(XC_SCENE_ABSENT);
    }
    // 错误路径：未登记画像挂接。
    if lk.attach(9, 1) != Err(XC_SCENE_ABSENT) {
        return Err(XC_SCENE_ABSENT);
    }
    Ok(0)
}
