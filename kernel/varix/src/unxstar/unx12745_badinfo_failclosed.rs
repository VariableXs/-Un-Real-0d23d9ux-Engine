//! UNX-F12745 坏 UNWIND_INFO 拒止压测（fail-closed） · 0x82440段
//!
//! 判据原文（派工卡 12745 逐字锚定）：
//!   UNX-F12746-J1 保存/恢复一致性压测 10/10，寄存器组逐段对平。
//!
//! 设计要点：
//! 1. UNWIND_INFO 头部字段闭集：Version/Flags/Frames/Size 四字段闭集——
//!    Version 只认 {1,2}，Flags 只认 {0,1,2,3}，Frames 非零且 ≤24，
//!    Size 必须与表体实长严格相等且 ≤64，任何越闭集值直接拒；
//! 2. 坏表五类注入样本：截断表/环表/越界偏移/非法 Version/Size 谎报，
//!    每类一个手工构造样本并绑定期望诊断码，判定器必须逐类拒止；
//! 3. 拒止判定器 judge_unwind_info(info) -> Result<ValidInfo, u32>：
//!    七道闸按 Version→Flags→Size→截断→条数→逐条结构→链路终结 定序，
//!    fail-closed：任何字段违约必回错误码，绝不 Ok；
//! 4. 拒止状态机 Clean→Suspect→Rejected 单调推进（Reject 后不可翻案）：
//!    Rejected 后连合法表也一并拒（整域封禁）；拒止日志容量耗尽即封禁；
//! 5. 注入器三类（Version 越闭集/Size 谎报/偏移越界）各 10 轮确定性注入，
//!    每轮必须被判定器以期望码抓住，逃脱即判据失效（EEscape）；
//! 6. 保存/恢复一致性：保存集（全表 SAVE 位图）与恢复集（链路可达 SAVE
//!    位图）逐段对平 10/10；双跑全流程指纹向量逐位一致（FNV-1a 32 位）；
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/.get_mut()/checked_*/saturating_*，无裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12745 − 12600) = 0x82440）。
pub const SEG_BASE: u32 = 0x82440;

// ── 拒止诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 非法 Version：越 {1,2} 两值闭集。
    EVersion,
    /// 非法 Flags：越 {0,1,2,3} 四值闭集。
    EFlags,
    /// Size 谎报：声明字节数与表体实长不符。
    ESizeLie,
    /// 截断表：表体字节数与 Frames×2 不符（短为截断、长为冗余）。
    ETrunc,
    /// 环表：跳接链在步数预算内收不到终结码。
    ECyclic,
    /// 越界偏移：SAVE 槽号或 LINK 目标越闭集。
    EOffset,
    /// Frames 条数违约：条数为零或越 24 上限。
    EFrames,
    /// 非法操作码：越 {0,1,2} 操作码闭集。
    EOp,
    /// 拒止域已 Rejected：不可翻案，合法表一并拒。
    ERejected,
    /// 拒止日志容量耗尽：日志满即整域封禁。
    EJournalCap,
    /// 状态机倒退：只进不退，倒退请求显性拒。
    ERewind,
    /// 注入逃脱：注入样本未被判定器以期望码抓住。
    EEscape,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EVersion => 0x01,
                Diag::EFlags => 0x02,
                Diag::ESizeLie => 0x03,
                Diag::ETrunc => 0x04,
                Diag::ECyclic => 0x05,
                Diag::EOffset => 0x06,
                Diag::EFrames => 0x07,
                Diag::EOp => 0x08,
                Diag::ERejected => 0x09,
                Diag::EJournalCap => 0x0a,
                Diag::ERewind => 0x0b,
                Diag::EEscape => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EVersion => "非法 Version：越 {1,2} 两值闭集，先核对头部版本域取值再送判",
            Diag::EFlags => "非法 Flags：越 {0,1,2,3} 四值闭集，只认 无/E/U/链 四旗，先压回闭集",
            Diag::ESizeLie => "Size 谎报：声明字节数与表体实长不等，先回读表体实长再申报 Size",
            Diag::ETrunc => "截断表：表体字节数 ≠ Frames×2（短为截断、长为冗余），补齐或裁齐后再判",
            Diag::ECyclic => "环表：跳接链在 24 步预算内收不到终结码，查 LINK 回边并剪环",
            Diag::EOffset => "越界偏移：SAVE 槽号越 16 寄存器闭集或 LINK 目标越条数界，先压回闭集",
            Diag::EFrames => "Frames 条数违约：条数为零或越 24 上限，核对头部条数域",
            Diag::EOp => "非法操作码：越 {0,1,2} 闭集（终结/保存/跳接），改用闭集码后重判",
            Diag::ERejected => "拒止域已 Rejected：不可翻案，合法表一并拒，走换域重启流程",
            Diag::EJournalCap => "拒止日志容量耗尽：日志满即整域封禁，先归档日志再重启判定域",
            Diag::ERewind => "状态机倒退：Clean→Suspect→Rejected 只进不退，倒退请求显性拒",
            Diag::EEscape => "注入逃脱：注入样本未被以期望码抓住，判定闸面失效须整面复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DIAG_ALL: &[Diag] = &[
    Diag::EVersion,
    Diag::EFlags,
    Diag::ESizeLie,
    Diag::ETrunc,
    Diag::ECyclic,
    Diag::EOffset,
    Diag::EFrames,
    Diag::EOp,
    Diag::ERejected,
    Diag::EJournalCap,
    Diag::ERewind,
    Diag::EEscape,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DIAG_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82441..0x8244C 闭集内，先核对段基址与偏移"
}

// ── 头部字段闭集常量（来源：UNWIND_INFO 头部字段惯例，工程闭集化冻结）──
/// Version 两值闭集 {1,2}（展开信息版本域现行值，冻结不许扩）。
pub const VERSION_CLOSED: &[u32] = &[1, 2];

/// Flags 四值闭集 {0,1,2,3}（无旗/E 旗/U 旗/链旗惯例，冻结不许扩）。
pub const FLAGS_CLOSED: &[u32] = &[0, 1, 2, 3];

/// 展开码条数上限（头部条数域工程上限：24 条，超出即拒）。
pub const FRAMES_CAP: u32 = 24;

/// 表体字节上限（表体 ≤ Frames×2 ≤ 48，64 为兜底上限，冻结值）。
pub const SIZE_CAP: u32 = 64;

/// x64 整数寄存器槽闭集（SAVE 槽号只认 0..16）。
pub const REG_SLOTS: u32 = 16;

/// 操作码闭集：0 = 终结码（链在此收口）。
pub const OP_STOP: u8 = 0;

/// 操作码闭集：1 = 保存寄存器（off 为寄存器槽号）。
pub const OP_SAVE: u8 = 1;

/// 操作码闭集：2 = 跳接下一帧（off 为目标条目索引）。
pub const OP_LINK: u8 = 2;

/// 拒止日志容量：日志满即整域封禁（fail-closed，冻结值）。
pub const JOURNAL_CAP: usize = 4;

/// 注入轮数（三类注入各 10 轮，判据口径冻结）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 域模型：原始信息与放行信息 ───────────────────────────────────
/// 原始 UNWIND_INFO：头部四字段闭集 + 表体字节序列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawInfo {
    /// 版本域（闭集 {1,2}）。
    pub version: u32,
    /// 旗域（闭集 {0,1,2,3}）。
    pub flags: u32,
    /// 展开码条数（声明值，非零且 ≤24）。
    pub frames: u32,
    /// 声明表体字节数（必须与 body.len() 严格相等）。
    pub size: u32,
    /// 表体：每条 2 字节（操作码 1 字节 + 偏移 1 字节）。
    pub body: Vec<u8>,
}

/// 放行信息：判定器七道闸全过后的可信产物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidInfo {
    /// 回读版本域（与原始一致）。
    pub version: u32,
    /// 回读旗域。
    pub flags: u32,
    /// 回读条数。
    pub frames: u32,
    /// 回读声明字节数。
    pub size: u32,
    /// 保存集位图：全表 SAVE 槽号逐位对平（bit n = 槽 n）。
    pub save_mask: u32,
    /// 链路步数：从条目 0 走到终结码的访问条数。
    pub chain_len: u32,
    /// FNV-1a 32 位指纹（双跑一致锚点）。
    pub fingerprint: u32,
}

/// FNV-1a 32 位指纹：头部四字段（小端逐字节）+ 表体全字节，确定性可复算。
pub fn fingerprint(info: &RawInfo) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in info.version.to_le_bytes().iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    for b in info.flags.to_le_bytes().iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    for b in info.frames.to_le_bytes().iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    for b in info.size.to_le_bytes().iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    for b in info.body.iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

/// 闭集成员判定（线性扫描，无裸下标）。
fn in_closed(v: u32, set: &[u32]) -> bool {
    for x in set.iter() {
        if *x == v {
            return true;
        }
    }
    false
}

// ── 拒止判定器：七道闸定序，fail-closed ─────────────────────────
/// 坏 UNWIND_INFO 拒止判定器。
/// 闸序：①Version → ②Flags → ③Size 谎报 → ④截断/冗余 → ⑤条数闭区间
/// → ⑥逐条结构（操作码/越界偏移） → ⑦链路终结（环表）。
/// fail-closed：任何字段违约必回错误码，绝不 Ok。
pub fn judge_unwind_info(info: &RawInfo) -> Result<ValidInfo, u32> {
    // 闸 1：Version 两值闭集 {1,2}。
    if !in_closed(info.version, VERSION_CLOSED) {
        return Err(Diag::EVersion.val());
    }
    // 闸 2：Flags 四值闭集 {0,1,2,3}。
    if !in_closed(info.flags, FLAGS_CLOSED) {
        return Err(Diag::EFlags.val());
    }
    // 闸 3：Size 谎报——声明字节数必须与表体实长严格相等。
    let body_len = info.body.len() as u32;
    if info.size != body_len {
        return Err(Diag::ESizeLie.val());
    }
    // 闸 4：截断/冗余——表体必须恰装 Frames×2 字节（短为截断、长为冗余）。
    let need = match info.frames.checked_mul(2) {
        Some(n) => n,
        None => return Err(Diag::EFrames.val()),
    };
    if body_len != need {
        return Err(Diag::ETrunc.val());
    }
    // 闸 5：Frames 条数闭区间——非零且不超容量。
    if info.frames == 0 || info.frames > FRAMES_CAP {
        return Err(Diag::EFrames.val());
    }
    // 防御兜底：条数闸后表体 ≤48 字节，Size 上限闸常闭，保留拦截面。
    if info.size > SIZE_CAP {
        return Err(Diag::ESizeLie.val());
    }
    // 闸 6：逐条结构——非法操作码 / 越界偏移（SAVE 槽号、LINK 目标）。
    let frames_usize = info.frames as usize;
    let mut save_mask: u32 = 0;
    let mut i: usize = 0;
    while i < frames_usize {
        let base = match i.checked_mul(2) {
            Some(b) => b,
            None => return Err(Diag::ETrunc.val()),
        };
        let op = match info.body.get(base) {
            Some(&b) => b,
            None => return Err(Diag::ETrunc.val()),
        };
        let off_pos = match base.checked_add(1) {
            Some(p) => p,
            None => return Err(Diag::ETrunc.val()),
        };
        let off = match info.body.get(off_pos) {
            Some(&b) => b,
            None => return Err(Diag::ETrunc.val()),
        };
        match op {
            OP_STOP => {}
            OP_SAVE => {
                let slot = off as u32;
                if slot >= REG_SLOTS {
                    return Err(Diag::EOffset.val());
                }
                save_mask |= 1u32 << slot;
            }
            OP_LINK => {
                if (off as usize) >= frames_usize {
                    return Err(Diag::EOffset.val());
                }
            }
            _ => return Err(Diag::EOp.val()),
        }
        i = i.saturating_add(1);
    }
    // 闸 7：链路终结——从条目 0 起步数预算（FRAMES_CAP）内必须收到终结码；
    // 环表（LINK 回边循环）与链走飞出表体（末条后无终结码）均在此拒。
    let mut steps: u32 = 0;
    let mut idx: usize = 0;
    loop {
        steps = steps.saturating_add(1);
        if steps > FRAMES_CAP {
            return Err(Diag::ECyclic.val());
        }
        let base = idx.saturating_mul(2);
        let op = match info.body.get(base) {
            Some(&b) => b,
            None => return Err(Diag::ECyclic.val()),
        };
        let off_pos = match base.checked_add(1) {
            Some(p) => p,
            None => return Err(Diag::ECyclic.val()),
        };
        let off = match info.body.get(off_pos) {
            Some(&b) => b,
            None => return Err(Diag::ECyclic.val()),
        };
        match op {
            OP_STOP => break,
            OP_SAVE => {
                // 闸 6 已验槽号 <16，此处位移安全；防御分支仍保留。
                idx = idx.saturating_add(1);
            }
            OP_LINK => {
                // 闸 6 已验目标 < frames，此处跳接安全；防御分支仍保留。
                idx = off as usize;
            }
            _ => return Err(Diag::EOp.val()),
        }
    }
    Ok(ValidInfo {
        version: info.version,
        flags: info.flags,
        frames: info.frames,
        size: info.size,
        save_mask,
        chain_len: steps,
        fingerprint: fingerprint(info),
    })
}

// ── 保存/恢复一致性对平（判据 J1 直读口） ────────────────────────
/// 保存/恢复一致性对平：保存集 = 全表 SAVE 位图（逐条扫描）；
/// 恢复集 = 链路可达 SAVE 位图（从条目 0 沿 保存/跳接/终结 语义走链）。
/// 两集逐段对平即寄存器组不丢不漏；返回 None = 表结构不齐，无法对平。
pub fn save_restore_pair(info: &RawInfo) -> Option<(u32, u32)> {
    let frames = info.frames as usize;
    // 保存集：全表逐条扫描 SAVE 槽号。
    let mut save: u32 = 0;
    let mut i: usize = 0;
    while i < frames {
        let base = i.checked_mul(2)?;
        let op = *info.body.get(base)?;
        let off_pos = base.checked_add(1)?;
        let off = *info.body.get(off_pos)? as u32;
        if op == OP_SAVE {
            if off >= REG_SLOTS {
                return None;
            }
            save |= 1u32.checked_shl(off)?;
        }
        i = i.saturating_add(1);
    }
    // 恢复集：链路可达 SAVE 位图（步数预算防环）。
    let mut restore: u32 = 0;
    let mut steps: u32 = 0;
    let mut idx: usize = 0;
    loop {
        steps = steps.saturating_add(1);
        if steps > FRAMES_CAP {
            return None;
        }
        let base = idx.checked_mul(2)?;
        let op = *info.body.get(base)?;
        let off_pos = base.checked_add(1)?;
        let off = *info.body.get(off_pos)? as u32;
        match op {
            OP_STOP => break,
            OP_SAVE => {
                if off >= REG_SLOTS {
                    return None;
                }
                restore |= 1u32.checked_shl(off)?;
                idx = idx.saturating_add(1);
            }
            OP_LINK => {
                if off as usize >= frames {
                    return None;
                }
                idx = off as usize;
            }
            _ => return None,
        }
    }
    Some((save, restore))
}

// ── 合法表构建器（确定性，无随机） ───────────────────────────────
/// 由种子导出条数（2..=5），构建器与注入器共用，防止口径漂移。
pub fn frames_of_seed(seed: u64) -> u32 {
    2u32.saturating_add((seed % 4) as u32)
}

/// 构建合法 UNWIND_INFO：线性 [SAVE×(n−1), STOP]，头部四字段闭集内。
/// 确定性：同种子同表，双跑一致。
pub fn build_valid_info(seed: u64) -> RawInfo {
    let frames = frames_of_seed(seed);
    let mut body: Vec<u8> = Vec::new();
    let mut i: u32 = 0;
    while i < frames {
        if i.saturating_add(1) == frames {
            // 末条：终结码，链在此收口。
            body.push(OP_STOP);
            body.push(0);
        } else {
            // 中间条：保存寄存器，槽号确定性取 0..16 闭集内。
            let slot = (seed as u32).wrapping_add(i.wrapping_mul(3)) % REG_SLOTS;
            body.push(OP_SAVE);
            body.push(slot as u8);
        }
        i = i.saturating_add(1);
    }
    RawInfo {
        version: 1,
        flags: 1,
        frames,
        size: body.len() as u32,
        body,
    }
}

// ── 坏表五类注入样本（手工构造，绑定期望诊断码） ─────────────────
/// 坏表样本：名称 + 原始信息 + 期望诊断码（判定器必须以此码拒止）。
pub struct BadSample {
    /// 样本名（诊断文本用）。
    pub name: &'static str,
    /// 坏表原始信息。
    pub info: RawInfo,
    /// 期望诊断码。
    pub expect: u32,
}

/// 坏表五类注入样本：截断表/环表/越界偏移/非法 Version/Size 谎报。
pub fn bad_samples() -> Vec<BadSample> {
    let mut out: Vec<BadSample> = Vec::new();
    // 一、截断表：Frames 声明条数超出表体实有条数（短装）。
    let mut trunc = build_valid_info(0x51);
    trunc.frames = trunc.frames.saturating_add(2);
    out.push(BadSample {
        name: "截断表",
        info: trunc,
        expect: Diag::ETrunc.val(),
    });
    // 二、环表：跳接链 0↔1 循环，步数预算内收不到终结码。
    let mut ring = RawInfo {
        version: 1,
        flags: 0,
        frames: 2,
        size: 4,
        body: Vec::new(),
    };
    ring.body.push(OP_LINK);
    ring.body.push(1);
    ring.body.push(OP_LINK);
    ring.body.push(0);
    out.push(BadSample {
        name: "环表",
        info: ring,
        expect: Diag::ECyclic.val(),
    });
    // 三、越界偏移：SAVE 槽号 200 越 16 寄存器闭集。
    let mut oob = build_valid_info(0x52);
    if let Some(slot) = oob.body.get_mut(1) {
        *slot = 200;
    }
    out.push(BadSample {
        name: "越界偏移",
        info: oob,
        expect: Diag::EOffset.val(),
    });
    // 四、非法 Version：7 越两值闭集。
    let mut ver = build_valid_info(0x53);
    ver.version = 7;
    out.push(BadSample {
        name: "非法Version",
        info: ver,
        expect: Diag::EVersion.val(),
    });
    // 五、Size 谎报：声明字节数比表体实长多 5。
    let mut lie = build_valid_info(0x54);
    lie.size = lie.size.saturating_add(5);
    out.push(BadSample {
        name: "Size谎报",
        info: lie,
        expect: Diag::ESizeLie.val(),
    });
    out
}

// ── 注入器：三类 × 10 轮，确定性注入 ─────────────────────────────
/// 注入类别闭集：Version 越闭集 / Size 谎报 / 偏移越界。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectionKind {
    /// Version 越两值闭集注入。
    VersionOob,
    /// Size 谎报注入（声明比实长多 1..10）。
    SizeLie,
    /// 偏移越界注入（SAVE 槽号压到 16..24）。
    OffsetOob,
}

impl InjectionKind {
    /// 类别中文名。
    pub fn name(self) -> &'static str {
        match self {
            InjectionKind::VersionOob => "Version越闭集",
            InjectionKind::SizeLie => "Size谎报",
            InjectionKind::OffsetOob => "偏移越界",
        }
    }
}

/// 构造第 round 轮注入样本：以合法表为底，按类别定点腐蚀一处字段。
/// 确定性：同类别同轮同样本，双跑一致。
pub fn inject(kind: InjectionKind, round: u32) -> RawInfo {
    let seed = SEG_BASE.wrapping_add(round) as u64;
    let mut info = build_valid_info(seed);
    match kind {
        InjectionKind::VersionOob => {
            // 越闭集候选表：0 与 3..=16 共十个越闭集值，逐轮轮换。
            const BAD_VERSIONS: [u32; 10] = [0, 3, 4, 5, 6, 7, 8, 9, 15, 16];
            info.version = match BAD_VERSIONS.get((round % 10) as usize) {
                Some(&v) => v,
                None => 0,
            };
        }
        InjectionKind::SizeLie => {
            // Size 谎报：声明比表体实长多 1..10 字节。
            info.size = (info.body.len() as u32)
                .saturating_add(round)
                .saturating_add(1);
        }
        InjectionKind::OffsetOob => {
            // 偏移越界：选一个 SAVE 条目（构建器保证前 frames−1 条为 SAVE），
            // 槽号压到 16..24，越寄存器闭集。
            let frames = frames_of_seed(seed);
            let save_cnt = (frames.saturating_sub(1).max(1)) as usize;
            let tgt = (round as usize) % save_cnt;
            let base = tgt.saturating_mul(2);
            let off_pos = match base.checked_add(1) {
                Some(p) => p,
                None => 0,
            };
            if let Some(slot) = info.body.get_mut(off_pos) {
                *slot = (REG_SLOTS as u8).wrapping_add((round % 8) as u8);
            }
        }
    }
    info
}

/// 单类注入扫荡：10 轮全部被判定器以期望码抓住才放行；
/// 任何一轮 Ok 或错码即注入逃脱，回 EEscape。
pub fn sweep_kind(kind: InjectionKind) -> Result<(u32, u32), u32> {
    let expect = match kind {
        InjectionKind::VersionOob => Diag::EVersion.val(),
        InjectionKind::SizeLie => Diag::ESizeLie.val(),
        InjectionKind::OffsetOob => Diag::EOffset.val(),
    };
    let mut caught: u32 = 0;
    let mut round: u32 = 0;
    while round < INJECT_ROUNDS {
        let bad = inject(kind, round);
        match judge_unwind_info(&bad) {
            Err(code) if code == expect => {
                caught = caught.saturating_add(1);
            }
            _ => return Err(Diag::EEscape.val()),
        }
        round = round.saturating_add(1);
    }
    Ok((caught, INJECT_ROUNDS))
}

// ── 拒止状态机：Clean→Suspect→Rejected 单调，Reject 后不可翻案 ──
/// 拒止状态机相位（只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectPhase {
    /// 清白：尚未见到坏表，合法表正常放行。
    Clean,
    /// 起疑：见过一例坏表，合法表仍放行，倒退请求显性拒。
    Suspect,
    /// 已拒：整域封禁，合法表一并拒，不可翻案。
    Rejected,
}

impl RejectPhase {
    /// 状态序（只许前进，倒退显性拒）。
    pub fn rank(self) -> u8 {
        match self {
            RejectPhase::Clean => 0,
            RejectPhase::Suspect => 1,
            RejectPhase::Rejected => 2,
        }
    }
}

/// 拒止域：状态机 + 拒止日志（容量封禁）。
pub struct Rejector {
    /// 当前相位。
    phase: RejectPhase,
    /// 累计送判样本数（含合法与坏表）。
    scanned: u32,
    /// 累计入账拒止数（与日志条数一致）。
    caught: u32,
    /// 拒止日志：每次入账拒止的诊断码，按序追加。
    journal: Vec<u32>,
}

impl Rejector {
    /// 开一个新拒止域（Clean，空日志）。
    pub fn new() -> Rejector {
        Rejector {
            phase: RejectPhase::Clean,
            scanned: 0,
            caught: 0,
            journal: Vec::new(),
        }
    }

    /// 当前相位。
    pub fn phase(&self) -> RejectPhase {
        self.phase
    }

    /// 累计送判样本数。
    pub fn scanned(&self) -> u32 {
        self.scanned
    }

    /// 累计入账拒止数。
    pub fn caught(&self) -> u32 {
        self.caught
    }

    /// 日志条数。
    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    /// 取第 idx 条日志（越界 None）。
    pub fn journal_at(&self, idx: usize) -> Option<u32> {
        self.journal.get(idx).copied()
    }

    /// 显性相位迁移：只许前进，倒退请求回 ERewind（单调守卫）。
    pub fn set_phase(&mut self, p: RejectPhase) -> Result<(), u32> {
        if p.rank() < self.phase.rank() {
            return Err(Diag::ERewind.val());
        }
        self.phase = p;
        Ok(())
    }

    /// 送判一个 UNWIND_INFO：
    /// 合法表——Clean/Suspect 放行，Rejected 封禁拒（不可翻案）；
    /// 坏表——入账日志并单调推进 Clean→Suspect→Rejected；
    /// 日志满——容量耗尽即整域封禁，回容量码（fail-closed）。
    pub fn inspect(&mut self, info: &RawInfo) -> Result<ValidInfo, u32> {
        self.scanned = self.scanned.saturating_add(1);
        match judge_unwind_info(info) {
            Ok(v) => {
                if self.phase.rank() >= RejectPhase::Rejected.rank() {
                    return Err(Diag::ERejected.val());
                }
                Ok(v)
            }
            Err(code) => {
                if self.journal.len() >= JOURNAL_CAP {
                    // 容量耗尽：封禁并拒，日志不再增长。
                    self.phase = RejectPhase::Rejected;
                    return Err(Diag::EJournalCap.val());
                }
                self.journal.push(code);
                self.caught = self.caught.saturating_add(1);
                // 单调推进：Clean→Suspect→Rejected，永不倒退。
                self.phase = match self.phase {
                    RejectPhase::Clean => RejectPhase::Suspect,
                    _ => RejectPhase::Rejected,
                };
                Err(code)
            }
        }
    }
}

// ── 双跑一致指纹：全流程指纹向量 ─────────────────────────────────
/// 全流程指纹向量：10 轮合法表指纹 + 三类注入 30 轮判码 + 五类坏表判码，
/// 共 45 项；确定性流程下双跑逐位一致。
pub fn pipeline_fingerprint_vec() -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    // 一、10 轮合法表指纹（Ok 记指纹，Err 记 0 占位——正常流不该发生）。
    let mut r: u32 = 0;
    while r < 10 {
        let info = build_valid_info(SEG_BASE.wrapping_add(r.wrapping_mul(0x11)) as u64);
        match judge_unwind_info(&info) {
            Ok(v) => out.push(v.fingerprint),
            Err(_) => out.push(0),
        }
        r = r.saturating_add(1);
    }
    // 二、三类注入 30 轮判码（Err 记诊断码，Ok 记 0——注入不该被放行）。
    let kinds = [
        InjectionKind::VersionOob,
        InjectionKind::SizeLie,
        InjectionKind::OffsetOob,
    ];
    for kind in kinds.iter() {
        let mut k: u32 = 0;
        while k < INJECT_ROUNDS {
            let bad = inject(*kind, k);
            match judge_unwind_info(&bad) {
                Err(code) => out.push(code),
                Ok(_) => out.push(0),
            }
            k = k.saturating_add(1);
        }
    }
    // 三、五类坏表样本判码（同上，拒码入向量）。
    for s in bad_samples().iter() {
        match judge_unwind_info(&s.info) {
            Err(code) => out.push(code),
            Ok(_) => out.push(0),
        }
    }
    out
}

// ── 判据谓词（私有，断言方向：cond 为违约条件，末行为成立条件） ──
/// 段位守卫：SEG 对齐 0x40 且落在卡定窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口，查表可命中。
fn diag_closed_ok() -> bool {
    for d in DIAG_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        let text = detail(v);
        if text.is_empty() || text != d.detail() {
            return false;
        }
    }
    // 未知码必须回落"未知"文案。
    let stray = detail(SEG_BASE.wrapping_add(0x3F));
    if !stray.contains("未知") {
        return false;
    }
    true
}

/// 头部四字段闭集守卫：Version/Flags 闭集内容冻结，容量关系自洽，操作码互异。
fn field_closed_ok() -> bool {
    // Version 闭集：恰 {1,2}。
    if VERSION_CLOSED.len() != 2 {
        return false;
    }
    if VERSION_CLOSED.first() != Some(&1) || VERSION_CLOSED.get(1) != Some(&2) {
        return false;
    }
    // Flags 闭集：恰 {0,1,2,3}。
    if FLAGS_CLOSED.len() != 4 {
        return false;
    }
    if FLAGS_CLOSED.first() != Some(&0)
        || FLAGS_CLOSED.get(1) != Some(&1)
        || FLAGS_CLOSED.get(2) != Some(&2)
        || FLAGS_CLOSED.get(3) != Some(&3)
    {
        return false;
    }
    // 容量关系：满容量表体（FRAMES_CAP×2）必须落在 Size 上限内。
    let need = match FRAMES_CAP.checked_mul(2) {
        Some(n) => n,
        None => 0,
    };
    if need == 0 || need > SIZE_CAP {
        return false;
    }
    // 寄存器槽闭集与操作码闭集：非空、互异、有界。
    if REG_SLOTS == 0 || REG_SLOTS > 32 {
        return false;
    }
    if OP_STOP != 0 || OP_SAVE == OP_STOP || OP_LINK == OP_STOP || OP_SAVE == OP_LINK {
        return false;
    }
    true
}

/// 正常流：合法表七道闸全过，ValidInfo 回读一致、链长对齐、位图非零。
fn happy_path_ok() -> bool {
    let info = build_valid_info(0xA5);
    match judge_unwind_info(&info) {
        Ok(v) => {
            if v.version != info.version || v.flags != info.flags {
                return false;
            }
            if v.frames != info.frames || v.size != info.size {
                return false;
            }
            // 线性表链路逐条走满：链长 = 条数。
            if v.chain_len != info.frames {
                return false;
            }
            if v.save_mask == 0 {
                return false;
            }
            // 指纹可复算：重算一次必须一致。
            v.fingerprint != 0 && v.fingerprint == fingerprint(&info)
        }
        Err(_) => false,
    }
}

/// 五类坏表全拒：每类样本被判定器以绑定期望码逐一拒止，绝不 Ok。
fn five_bad_rejected_ok() -> bool {
    let samples = bad_samples();
    if samples.len() != 5 {
        return false;
    }
    for s in samples.iter() {
        match judge_unwind_info(&s.info) {
            Err(code) => {
                if code != s.expect {
                    return false;
                }
            }
            Ok(_) => return false,
        }
    }
    true
}

/// 判定次序：多重违约样本按闸序返回最前闸错误码（六路定序锚点）。
fn judge_order_ok() -> bool {
    // (a) Version 与 Size 双违 → 最前闸 Version。
    let mut a = build_valid_info(0x61);
    a.version = 9;
    a.size = a.size.saturating_add(3);
    if judge_unwind_info(&a) != Err(Diag::EVersion.val()) {
        return false;
    }
    // (b) Version 与 Flags 双违 → Version 先于 Flags。
    let mut b = build_valid_info(0x62);
    b.version = 0;
    b.flags = 9;
    if judge_unwind_info(&b) != Err(Diag::EVersion.val()) {
        return false;
    }
    // (c) Size 谎报与截断双违 → Size 先于截断。
    let mut c = build_valid_info(0x63);
    c.frames = c.frames.saturating_add(6);
    c.size = c.size.saturating_add(1);
    if judge_unwind_info(&c) != Err(Diag::ESizeLie.val()) {
        return false;
    }
    // (d) 截断与条数超容双违 → 截断先于条数闸。
    let mut d = RawInfo {
        version: 1,
        flags: 0,
        frames: 30,
        size: 2,
        body: Vec::new(),
    };
    d.body.push(OP_SAVE);
    d.body.push(1);
    if judge_unwind_info(&d) != Err(Diag::ETrunc.val()) {
        return false;
    }
    // (e) 非法操作码与越界偏移同条 → 操作码闸先于偏移闸。
    let mut e = RawInfo {
        version: 1,
        flags: 0,
        frames: 1,
        size: 2,
        body: Vec::new(),
    };
    e.body.push(9);
    e.body.push(200);
    if judge_unwind_info(&e) != Err(Diag::EOp.val()) {
        return false;
    }
    // (f) 越界偏移与环表双违 → 逐条闸先于链路闸。
    let mut f = RawInfo {
        version: 1,
        flags: 0,
        frames: 2,
        size: 4,
        body: Vec::new(),
    };
    f.body.push(OP_SAVE);
    f.body.push(99);
    f.body.push(OP_LINK);
    f.body.push(0);
    if judge_unwind_info(&f) != Err(Diag::EOffset.val()) {
        return false;
    }
    true
}

/// 注入一：Version 越闭集 10 轮全抓。
fn inject_version_ok() -> bool {
    match sweep_kind(InjectionKind::VersionOob) {
        Ok((caught, total)) => caught == INJECT_ROUNDS && total == INJECT_ROUNDS,
        Err(_) => false,
    }
}

/// 注入二：Size 谎报 10 轮全抓。
fn inject_size_ok() -> bool {
    match sweep_kind(InjectionKind::SizeLie) {
        Ok((caught, total)) => caught == INJECT_ROUNDS && total == INJECT_ROUNDS,
        Err(_) => false,
    }
}

/// 注入三：偏移越界 10 轮全抓。
fn inject_offset_ok() -> bool {
    match sweep_kind(InjectionKind::OffsetOob) {
        Ok((caught, total)) => caught == INJECT_ROUNDS && total == INJECT_ROUNDS,
        Err(_) => false,
    }
}

/// 状态机单调：Clean→Suspect→Rejected 只进不退，Reject 后不可翻案。
fn state_machine_ok() -> bool {
    let mut r = Rejector::new();
    if r.phase() != RejectPhase::Clean {
        return false;
    }
    // 合法表放行，相位不动。
    let good = build_valid_info(0x71);
    if r.inspect(&good).is_err() {
        return false;
    }
    if r.phase() != RejectPhase::Clean {
        return false;
    }
    // 第一例坏表：Clean→Suspect。
    let mut bad1 = build_valid_info(0x72);
    bad1.version = 9;
    if r.inspect(&bad1).is_ok() {
        return false;
    }
    if r.phase() != RejectPhase::Suspect {
        return false;
    }
    // Suspect 仍受理合法表（尚未封禁）。
    if r.inspect(&good).is_err() {
        return false;
    }
    // 第二例坏表：Suspect→Rejected。
    let mut bad2 = build_valid_info(0x73);
    bad2.size = bad2.size.saturating_add(2);
    if r.inspect(&bad2).is_ok() {
        return false;
    }
    if r.phase() != RejectPhase::Rejected {
        return false;
    }
    // Reject 后不可翻案：合法表也拒，且回封禁码。
    if r.inspect(&good) != Err(Diag::ERejected.val()) {
        return false;
    }
    // 送判计数对账：合法×3 + 坏×2 = 5。
    if r.scanned() != 5 || r.caught() != 2 {
        return false;
    }
    // 显性倒退拒：Suspect→Clean、Rejected→Suspect 均拒；前进放行。
    let mut r2 = Rejector::new();
    if r2.set_phase(RejectPhase::Suspect).is_err() {
        return false;
    }
    if r2.set_phase(RejectPhase::Clean) != Err(Diag::ERewind.val()) {
        return false;
    }
    if r2.set_phase(RejectPhase::Rejected).is_err() {
        return false;
    }
    if r2.set_phase(RejectPhase::Suspect) != Err(Diag::ERewind.val()) {
        return false;
    }
    true
}

/// 容量耗尽：日志满前逐例入账，满后即整域封禁回容量码（fail-closed）。
fn journal_cap_ok() -> bool {
    let mut r = Rejector::new();
    // 连投 JOURNAL_CAP+1 例坏表（Flags 越闭集，回 EFlags）。
    let mut n: u32 = 0;
    while n <= JOURNAL_CAP as u32 {
        let mut bad = build_valid_info(SEG_BASE.wrapping_add(0x80 + n) as u64);
        bad.flags = 9;
        let _ = r.inspect(&bad);
        n = n.saturating_add(1);
    }
    // 前 JOURNAL_CAP 例入账，容量耗尽那例回容量码、不入账。
    if r.journal_len() != JOURNAL_CAP {
        return false;
    }
    if r.journal_at(0) != Some(Diag::EFlags.val()) {
        return false;
    }
    if r.phase() != RejectPhase::Rejected {
        return false;
    }
    if r.caught() != JOURNAL_CAP as u32 {
        return false;
    }
    // 封禁后合法表也拒（不可翻案优先于受理）。
    let good = build_valid_info(0x81);
    r.inspect(&good) == Err(Diag::ERejected.val())
}

/// 双跑一致：两次全流程指纹向量逐位一致，且恒 45 项。
fn dual_run_ok() -> bool {
    let a = pipeline_fingerprint_vec();
    let b = pipeline_fingerprint_vec();
    if a.len() != 45 || b.len() != 45 {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a.get(i) != b.get(i) {
            return false;
        }
        i = i.saturating_add(1);
    }
    true
}

/// 保存/恢复一致性压测 10/10：每轮合法表保存集==恢复集且与判定器位图
/// 一致；另验错接链样本（链外保存寄存器被恢复集漏收）必被对平检测抓住。
fn save_restore_ok() -> bool {
    // 10/10 轮逐段对平。
    let mut rounds: u32 = 0;
    let mut r: u32 = 0;
    while r < 10 {
        let info = build_valid_info(SEG_BASE.wrapping_add(r.wrapping_mul(0x11)) as u64);
        match judge_unwind_info(&info) {
            Ok(v) => match save_restore_pair(&info) {
                Some((save, restore)) => {
                    if save != restore {
                        return false;
                    }
                    if v.save_mask != save {
                        return false;
                    }
                    rounds = rounds.saturating_add(1);
                }
                None => return false,
            },
            Err(_) => return false,
        }
        r = r.saturating_add(1);
    }
    if rounds != 10 {
        return false;
    }
    // 错接链：恢复集沿链漏收链外 SAVE（槽 5），对平检测必须抓出差集。
    let mut mis = RawInfo {
        version: 1,
        flags: 0,
        frames: 4,
        size: 8,
        body: Vec::new(),
    };
    mis.body.push(OP_SAVE);
    mis.body.push(3);
    mis.body.push(OP_LINK);
    mis.body.push(3);
    mis.body.push(OP_SAVE);
    mis.body.push(5);
    mis.body.push(OP_STOP);
    mis.body.push(0);
    match save_restore_pair(&mis) {
        Some((save, restore)) => save != restore,
        None => false,
    }
}

/// 收口谓词：13 条判据谓词全绿才许收口。
fn all_preds_ok() -> bool {
    seg_guard_ok()
        && diag_closed_ok()
        && field_closed_ok()
        && happy_path_ok()
        && five_bad_rejected_ok()
        && judge_order_ok()
        && inject_version_ok()
        && inject_size_ok()
        && inject_offset_ok()
        && state_machine_ok()
        && journal_cap_ok()
        && dual_run_ok()
        && save_restore_ok()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12745 自检（判据 12745 卡：坏 UNWIND_INFO 拒止压测 fail-closed）。
pub fn run_unx12745_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12745");

    s.add_dyn(
        "F12745-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05x} align40={} win=[0x80000,0x831C0]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    s.add_dyn(
        "F12745-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x} last=0x{:05x}",
            DIAG_ALL.len(),
            DIAG_ALL.first().map(|d| d.val()).unwrap_or(0),
            DIAG_ALL.last().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12745-03 头部四字段闭集",
        field_closed_ok(),
        &format!(
            "ver={} flags={} frames_cap={} size_cap={} reg={}",
            VERSION_CLOSED.len(),
            FLAGS_CLOSED.len(),
            FRAMES_CAP,
            SIZE_CAP,
            REG_SLOTS
        ),
    );

    let good = build_valid_info(0xA5);
    s.add_dyn(
        "F12745-04 正常流判定放行",
        happy_path_ok(),
        &format!(
            "frames={} size={} judge_ok={}",
            good.frames,
            good.size,
            judge_unwind_info(&good).is_ok()
        ),
    );

    // 五类坏表名单动态串（逐类列名）。
    let mut names = String::new();
    for sp in bad_samples().iter() {
        if !names.is_empty() {
            names.push_str("/");
        }
        names.push_str(sp.name);
    }
    s.add_dyn(
        "F12745-05 五类坏表全拒",
        five_bad_rejected_ok(),
        &format!("{} 五类期望码逐一拒止，绝不 Ok", names),
    );

    s.add_dyn(
        "F12745-06 判定次序定序",
        judge_order_ok(),
        "多重违约样本六路：最前闸错误码先行（Version>Flags>Size>截断>逐条>链路）",
    );

    let (v_caught, v_total) = match sweep_kind(InjectionKind::VersionOob) {
        Ok(pair) => pair,
        Err(_) => (0, INJECT_ROUNDS),
    };
    s.add_dyn(
        "F12745-07 注入一 Version越闭集 10轮全抓",
        v_caught == INJECT_ROUNDS && v_total == INJECT_ROUNDS,
        &format!(
            "kind={} caught={}/{}",
            InjectionKind::VersionOob.name(),
            v_caught,
            v_total
        ),
    );

    let (s_caught, s_total) = match sweep_kind(InjectionKind::SizeLie) {
        Ok(pair) => pair,
        Err(_) => (0, INJECT_ROUNDS),
    };
    s.add_dyn(
        "F12745-08 注入二 Size谎报 10轮全抓",
        s_caught == INJECT_ROUNDS && s_total == INJECT_ROUNDS,
        &format!(
            "kind={} caught={}/{}",
            InjectionKind::SizeLie.name(),
            s_caught,
            s_total
        ),
    );

    let (o_caught, o_total) = match sweep_kind(InjectionKind::OffsetOob) {
        Ok(pair) => pair,
        Err(_) => (0, INJECT_ROUNDS),
    };
    s.add_dyn(
        "F12745-09 注入三 偏移越界 10轮全抓",
        o_caught == INJECT_ROUNDS && o_total == INJECT_ROUNDS,
        &format!(
            "kind={} caught={}/{}",
            InjectionKind::OffsetOob.name(),
            o_caught,
            o_total
        ),
    );

    s.add_dyn(
        "F12745-10 状态机单调不可翻案",
        state_machine_ok(),
        "Clean→Suspect→Rejected 只进不退；Rejected 后合法表一并拒；倒退请求显性拒",
    );

    s.add_dyn(
        "F12745-11 拒止日志容量耗尽",
        journal_cap_ok(),
        &format!(
            "cap={} 满前逐例入账，满后整域封禁回容量码",
            JOURNAL_CAP
        ),
    );

    s.add_dyn(
        "F12745-12 双跑一致指纹",
        dual_run_ok(),
        &format!(
            "两次全流程指纹向量各 {} 项逐位一致（10 合法 + 30 注入 + 5 坏表）",
            pipeline_fingerprint_vec().len()
        ),
    );

    s.add_dyn(
        "F12745-13 保存/恢复一致性压测 10/10",
        save_restore_ok(),
        "寄存器组逐段对平：保存集==恢复集 10/10，错接链样本必被抓",
    );

    // 收口：判据 13 条声明，全绿才收口。
    s.add_dyn(
        "F12745-14 收口：判据 13 条声明",
        all_preds_ok(),
        &format!("preds=13 green={}", all_preds_ok()),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在卡定窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82440);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(DIAG_ALL.len(), 12);
        let mut i = 0usize;
        while i < DIAG_ALL.len() {
            let vi = DIAG_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < DIAG_ALL.len() {
                let vj = DIAG_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
        assert_eq!(
            detail(Diag::ECyclic.val()),
            Diag::ECyclic.detail()
        );
    }

    #[test]
    fn happy_path_judge_ok() {
        // 正常流：合法表放行，回读一致，指纹可复算。
        let info = build_valid_info(0xA5);
        let v = match judge_unwind_info(&info) {
            Ok(v) => v,
            Err(e) => {
                assert_eq!(e, 0, "合法表被拒：0x{:05x}", e);
                return;
            }
        };
        assert_eq!(v.version, info.version);
        assert_eq!(v.flags, info.flags);
        assert_eq!(v.frames, info.frames);
        assert_eq!(v.size, info.size);
        assert_eq!(v.chain_len, info.frames);
        assert_eq!(v.save_mask, fingerprint_mask_of(&info));
        assert_eq!(v.fingerprint, fingerprint(&info));
        assert!(v.fingerprint != 0);
    }

    /// 测试辅助：重算合法表 SAVE 位图（独立于判定器，防同源自证）。
    fn fingerprint_mask_of(info: &RawInfo) -> u32 {
        let mut m = 0u32;
        let mut k = 0usize;
        while k < info.frames as usize {
            let base = k * 2;
            let op = info.body.get(base).copied().unwrap_or(0);
            let off = info.body.get(base + 1).copied().unwrap_or(0) as u32;
            if op == OP_SAVE && off < REG_SLOTS {
                m |= 1u32 << off;
            }
            k += 1;
        }
        m
    }

    #[test]
    fn five_bad_samples_all_rejected_with_exact_codes() {
        // 五类坏表逐类以期望码拒止，绝不 Ok。
        let samples = bad_samples();
        assert_eq!(samples.len(), 5);
        for s in samples.iter() {
            assert_eq!(
                judge_unwind_info(&s.info),
                Err(s.expect),
                "样本 {} 未以期望码拒止",
                s.name
            );
        }
        // 附加：Flags 越闭集单独一路被拒。
        let mut bad_flags = build_valid_info(0x56);
        bad_flags.flags = 4;
        assert_eq!(judge_unwind_info(&bad_flags), Err(Diag::EFlags.val()));
    }

    #[test]
    fn judge_gate_order_fixed() {
        // 判定次序：多重违约样本按闸序返回最前闸错误码（六路）。
        assert!(judge_order_ok());
        // (a) Version 与 Size 双违 → Version。
        let mut a = build_valid_info(0x61);
        a.version = 9;
        a.size = a.size.saturating_add(3);
        assert_eq!(judge_unwind_info(&a), Err(Diag::EVersion.val()));
        // (d) 截断与条数超容双违 → 截断。
        let mut d = RawInfo {
            version: 1,
            flags: 0,
            frames: 30,
            size: 2,
            body: Vec::new(),
        };
        d.body.push(OP_SAVE);
        d.body.push(1);
        assert_eq!(judge_unwind_info(&d), Err(Diag::ETrunc.val()));
        // (f) 越界偏移与环表双违 → 逐条闸先于链路闸。
        let mut f = RawInfo {
            version: 1,
            flags: 0,
            frames: 2,
            size: 4,
            body: Vec::new(),
        };
        f.body.push(OP_SAVE);
        f.body.push(99);
        f.body.push(OP_LINK);
        f.body.push(0);
        assert_eq!(judge_unwind_info(&f), Err(Diag::EOffset.val()));
    }

    #[test]
    fn three_injections_ten_rounds_all_caught() {
        // 三类注入各 10 轮全抓，无逃脱。
        assert_eq!(
            sweep_kind(InjectionKind::VersionOob),
            Ok((INJECT_ROUNDS, INJECT_ROUNDS))
        );
        assert_eq!(
            sweep_kind(InjectionKind::SizeLie),
            Ok((INJECT_ROUNDS, INJECT_ROUNDS))
        );
        assert_eq!(
            sweep_kind(InjectionKind::OffsetOob),
            Ok((INJECT_ROUNDS, INJECT_ROUNDS))
        );
        // 注入样本逐轮验证期望码（确定性抽查）。
        let bad_v = inject(InjectionKind::VersionOob, 3);
        assert_eq!(judge_unwind_info(&bad_v), Err(Diag::EVersion.val()));
        let bad_s = inject(InjectionKind::SizeLie, 7);
        assert_eq!(judge_unwind_info(&bad_s), Err(Diag::ESizeLie.val()));
        let bad_o = inject(InjectionKind::OffsetOob, 5);
        assert_eq!(judge_unwind_info(&bad_o), Err(Diag::EOffset.val()));
    }

    #[test]
    fn dual_run_identical_fingerprints() {
        // 双跑一致：两次全流程指纹向量恒 45 项且逐位一致。
        let a = pipeline_fingerprint_vec();
        let b = pipeline_fingerprint_vec();
        assert_eq!(a.len(), 45);
        assert_eq!(a, b);
        // 注入器本身也双跑一致。
        assert_eq!(
            inject(InjectionKind::SizeLie, 4),
            inject(InjectionKind::SizeLie, 4)
        );
    }

    #[test]
    fn boundaries_four_kinds() {
        // 边界一：Version 1/2 收，0/3 拒。
        let mut v1 = build_valid_info(0x91);
        v1.version = 1;
        assert!(judge_unwind_info(&v1).is_ok());
        let mut v2 = build_valid_info(0x92);
        v2.version = 2;
        assert!(judge_unwind_info(&v2).is_ok());
        let mut v0 = build_valid_info(0x93);
        v0.version = 0;
        assert_eq!(judge_unwind_info(&v0), Err(Diag::EVersion.val()));
        let mut v3 = build_valid_info(0x94);
        v3.version = 3;
        assert_eq!(judge_unwind_info(&v3), Err(Diag::EVersion.val()));
        // 边界二：SAVE 槽 15 收，16 拒。
        let mut slot15 = RawInfo {
            version: 1,
            flags: 0,
            frames: 2,
            size: 4,
            body: Vec::new(),
        };
        slot15.body.push(OP_SAVE);
        slot15.body.push(15);
        slot15.body.push(OP_STOP);
        slot15.body.push(0);
        match judge_unwind_info(&slot15) {
            Ok(v) => assert_eq!(v.save_mask, 0x8000),
            Err(e) => assert!(false, "槽 15 应放行：0x{:05x}", e),
        }
        let mut slot16 = slot15.clone();
        if let Some(b) = slot16.body.get_mut(1) {
            *b = 16;
        }
        assert_eq!(judge_unwind_info(&slot16), Err(Diag::EOffset.val()));
        // 边界三：Frames 24 收（23 保存 + 终结），25 拒，0 拒。
        let mut big = RawInfo {
            version: 2,
            flags: 3,
            frames: 24,
            size: 48,
            body: Vec::new(),
        };
        let mut k = 0u32;
        while k < 23 {
            big.body.push(OP_SAVE);
            big.body.push((k % REG_SLOTS) as u8);
            k += 1;
        }
        big.body.push(OP_STOP);
        big.body.push(0);
        match judge_unwind_info(&big) {
            Ok(v) => {
                assert_eq!(v.chain_len, 24);
                assert_eq!(v.save_mask, 0xFFFF);
            }
            Err(e) => assert!(false, "24 条满容量表应放行：0x{:05x}", e),
        }
        let mut over = big.clone();
        over.frames = 25;
        over.size = 50;
        over.body.push(OP_STOP);
        over.body.push(0);
        assert_eq!(judge_unwind_info(&over), Err(Diag::EFrames.val()));
        let mut zero = build_valid_info(0x95);
        zero.frames = 0;
        zero.size = 0;
        zero.body.clear();
        assert_eq!(judge_unwind_info(&zero), Err(Diag::EFrames.val()));
        // 边界四：表体长度恰齐收，多一字拒（截断/冗余闸）。
        let mut extra = build_valid_info(0x96);
        let old_size = extra.size;
        extra.body.push(0);
        extra.size = old_size.saturating_add(1);
        assert_eq!(judge_unwind_info(&extra), Err(Diag::ETrunc.val()));
    }

    #[test]
    fn journal_capacity_exhausted_fails_closed() {
        // 容量耗尽：日志满前逐例入账，满后回容量码并整域封禁。
        assert!(journal_cap_ok());
        let mut r = Rejector::new();
        let mut n = 0u32;
        while n <= JOURNAL_CAP as u32 {
            let mut bad = build_valid_info(SEG_BASE.wrapping_add(0x80 + n) as u64);
            bad.flags = 9;
            let _ = r.inspect(&bad);
            n += 1;
        }
        assert_eq!(r.journal_len(), JOURNAL_CAP);
        assert_eq!(r.journal_at(0), Some(Diag::EFlags.val()));
        assert_eq!(r.phase(), RejectPhase::Rejected);
        let good = build_valid_info(0x81);
        assert_eq!(r.inspect(&good), Err(Diag::ERejected.val()));
    }

    #[test]
    fn state_machine_monotonic_no_rewind() {
        // 状态机单调：Clean→Suspect→Rejected，Reject 后不可翻案。
        assert!(state_machine_ok());
        assert!(RejectPhase::Clean.rank() < RejectPhase::Suspect.rank());
        assert!(RejectPhase::Suspect.rank() < RejectPhase::Rejected.rank());
        let mut r = Rejector::new();
        let mut bad = build_valid_info(0x74);
        bad.version = 9;
        assert!(r.inspect(&bad).is_err());
        assert_eq!(r.phase(), RejectPhase::Suspect);
        assert_eq!(r.set_phase(RejectPhase::Clean), Err(Diag::ERewind.val()));
        assert!(r.set_phase(RejectPhase::Rejected).is_ok());
        assert_eq!(r.set_phase(RejectPhase::Suspect), Err(Diag::ERewind.val()));
    }

    #[test]
    fn save_restore_pair_ten_rounds_and_mislink_caught() {
        // 保存/恢复一致性 10/10：保存集==恢复集，错接链必被抓。
        assert!(save_restore_ok());
        let mut r = 0u32;
        while r < 10 {
            let info = build_valid_info(SEG_BASE.wrapping_add(r.wrapping_mul(0x11)) as u64);
            match save_restore_pair(&info) {
                Some((save, restore)) => {
                    assert_eq!(save, restore, "第 {} 轮寄存器组不对平", r);
                    assert!(save != 0);
                }
                None => assert!(false, "第 {} 轮合法表无法对平", r),
            }
            r += 1;
        }
        // 错接链：恢复集漏收链外 SAVE（槽 5），差集必现。
        let mut mis = RawInfo {
            version: 1,
            flags: 0,
            frames: 4,
            size: 8,
            body: Vec::new(),
        };
        mis.body.push(OP_SAVE);
        mis.body.push(3);
        mis.body.push(OP_LINK);
        mis.body.push(3);
        mis.body.push(OP_SAVE);
        mis.body.push(5);
        mis.body.push(OP_STOP);
        mis.body.push(0);
        match save_restore_pair(&mis) {
            Some((save, restore)) => {
                assert_ne!(save, restore);
                assert_eq!(save, 0x28);
                assert_eq!(restore, 0x08);
            }
            None => assert!(false, "错接链样本应可对平（并报差集）"),
        }
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿、条数 ≥12（含收口）。
        let s = run_unx12745_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
        assert_eq!(s.tally().0, s.len());
    }
}
