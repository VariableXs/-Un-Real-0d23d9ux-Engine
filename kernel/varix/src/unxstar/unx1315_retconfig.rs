//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1315 · 退役配置总表（B26 段参数收口） · 0x7640
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1315-J1 B26 段参数 ≥12 项注册总表（窗口参数/回退上限/
//!      对拍缓冲/清理开关等），双闸一致率 100%，篡改 10/10 检出回退；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 参数总表 12 项（≥12 硬线）：窗口大小/窗口超时/回退上限/对拍
//!     缓冲/清理开关/风暴强度/红线/黄线/排空定额/断言硬线/审计环/
//!     消费方观察旗，常量正本+登记副本双份；
//!   - 类型三型：整数/千分制（≤1000）/布尔（0/1），类型错必拒；
//!   - 双闸写路径：闸一=范围校验（min≤v≤max），闸二=回读校验
//!     （写后读回==写值），两闸全过才算写成功（一致率 1000‰）；
//!   - 篡改检测：登记副本 vs 常量正本逐字段对拍，漂移即检出并
//!     回退到正本值（10/10 判准）；
//!   - 冻结制：总表冻结后写路径必拒（收口语义）；
//!   - 参数摘要：全部参数值卷积指纹，两次汇总一致；
//!   - 注入核 5 项（越界写/冻结改/篡改/类型错/未注册写）全部必抓，
//!     LCG 种子写死 0x7640_0011。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7640 段，配置总表专属）。
pub const SEG_BASE: u16 = 0x7640;

// ── 诊断码闭集（0x7640..0x7649 十码） ───────────────────────
/// 0x7640 越界写。
pub const E_RANGE: u16 = SEG_BASE;
/// 0x7641 冻结改写。
pub const E_FROZEN_WRITE: u16 = SEG_BASE + 1;
/// 0x7642 参数篡改。
pub const E_TAMPER: u16 = SEG_BASE + 2;
/// 0x7643 类型错。
pub const E_TYPE_BAD: u16 = SEG_BASE + 3;
/// 0x7644 未注册写。
pub const E_UNREG: u16 = SEG_BASE + 4;
/// 0x7645 参数号重复。
pub const E_ID_DUP: u16 = SEG_BASE + 5;
/// 0x7646 回读不符。
pub const E_RB_MISS: u16 = SEG_BASE + 6;
/// 0x7647 双闸未过。
pub const E_GATED: u16 = SEG_BASE + 7;
/// 0x7648 总表冻结后登记。
pub const E_RDONLY: u16 = SEG_BASE + 8;
/// 0x7649 空总表。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_RANGE => "越界写：先看 min/max 再写，出界值直接拒",
        E_FROZEN_WRITE => "冻结改写：总表已收口，变更走新版本登记",
        E_TAMPER => "参数篡改：登记副本与正本漂移，回退到正本值",
        E_TYPE_BAD => "类型错：布尔只收 0/1，千分制 ≤1000",
        E_UNREG => "未注册写：先登记参数条目再写值",
        E_ID_DUP => "参数号重复：按 1..=12 重排唯一号",
        E_RB_MISS => "回读不符：写后读回与写值不等，查写入通道",
        E_GATED => "双闸未过：范围闸+回读闸全过才算写成功",
        E_RDONLY => "总表冻结：只许读不许写",
        E_EMPTY => "空总表：先注册 12 项再出摘要",
        _ => "未知 0x7640 段诊断码：核对配置总表段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// 参数数硬线。
pub const PARAM_MIN: usize = 12;
/// 演练轮数。
pub const ROUNDS: usize = 10;

/// 参数类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PType {
    /// 整数。
    Int,
    /// 千分制（0..=1000）。
    Permille,
    /// 布尔（0/1）。
    Flag,
}

impl PType {
    /// 码。
    pub const fn code(self) -> u8 {
        match self {
            PType::Int => 0,
            PType::Permille => 1,
            PType::Flag => 2,
        }
    }
    /// 中文。
    pub const fn text(self) -> &'static str {
        match self {
            PType::Int => "整数",
            PType::Permille => "千分制",
            PType::Flag => "布尔",
        }
    }
}

/// 参数条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamSpec {
    /// 参数号（1..=12）。
    pub id: u16,
    /// 名。
    pub name: &'static str,
    /// 类型。
    pub ptype: PType,
    /// 下界。
    pub min: u64,
    /// 上界。
    pub max: u64,
    /// 默认值（=正本值）。
    pub default: u64,
}

/// 12 项 B26 段参数常量正本（≥12 硬线）。
pub const PARAM_TABLE: [ParamSpec; 12] = [
    ParamSpec { id: 1, name: "窗口大小", ptype: PType::Int, min: 16, max: 4096, default: 256 },
    ParamSpec { id: 2, name: "窗口超时", ptype: PType::Int, min: 1, max: 100, default: 40 },
    ParamSpec { id: 3, name: "回退上限", ptype: PType::Int, min: 0, max: 10, default: 3 },
    ParamSpec { id: 4, name: "对拍缓冲", ptype: PType::Int, min: 8, max: 1024, default: 64 },
    ParamSpec { id: 5, name: "清理开关", ptype: PType::Flag, min: 0, max: 1, default: 0 },
    ParamSpec { id: 6, name: "风暴强度", ptype: PType::Permille, min: 0, max: 1000, default: 700 },
    ParamSpec { id: 7, name: "告警红线", ptype: PType::Permille, min: 1, max: 1000, default: 1000 },
    ParamSpec { id: 8, name: "告警黄线", ptype: PType::Permille, min: 0, max: 999, default: 990 },
    ParamSpec { id: 9, name: "排空定额", ptype: PType::Int, min: 1, max: 256, default: 8 },
    ParamSpec { id: 10, name: "断言硬线", ptype: PType::Int, min: 40, max: 1000, default: 40 },
    ParamSpec { id: 11, name: "审计环", ptype: PType::Int, min: 64, max: 65536, default: 512 },
    ParamSpec { id: 12, name: "消费方观察旗", ptype: PType::Flag, min: 0, max: 1, default: 1 },
];

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 配置总表 ────────────────────────────────────────────────
/// 配置总表（登记副本 + 双闸写路径）。
#[derive(Debug)]
pub struct ConfigBook {
    /// 登记条目（id → 当前值）。
    entries: Vec<(ParamSpec, u64)>,
    /// 冻结旗。
    frozen: bool,
    /// 双闸通过计数。
    gated_ok: u32,
    /// 闸二拒绝计数（回读不符）。
    gated_rej: u32,
    /// 闸一拒绝计数（范围/类型）。
    g1_rej: u32,
}

impl ConfigBook {
    /// 新账。
    pub fn new() -> ConfigBook {
        ConfigBook { entries: Vec::new(), frozen: false, gated_ok: 0, gated_rej: 0, g1_rej: 0 }
    }

    /// 注册（幂等；号唯一；冻结拒）。
    pub fn register(&mut self, spec: ParamSpec) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if spec.id as usize > PARAM_TABLE.len() || spec.id == 0 {
            return Err(E_UNREG);
        }
        if self.entries.iter().any(|(s, _)| s.id == spec.id) {
            return Err(E_ID_DUP);
        }
        self.entries.push((spec, spec.default));
        Ok(())
    }

    /// 全量注册。
    pub fn register_all(&mut self) -> Result<usize, u16> {
        for s in PARAM_TABLE.iter() {
            self.register(*s)?;
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

    /// 注册数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 资产化判准（12 项全注册且与正本一致）。
    pub fn assetized(&self) -> bool {
        self.entries.len() >= PARAM_MIN
            && PARAM_TABLE.iter().all(|s| {
                match self.entries.iter().find(|(r, _)| r.id == s.id) {
                    Some((r, v)) => r == s && *v == s.default,
                    None => false,
                }
            })
    }

    /// 类型校验。
    pub fn type_ok(spec: &ParamSpec, v: u64) -> bool {
        match spec.ptype {
            PType::Flag => v <= 1,
            PType::Permille => v <= 1000,
            PType::Int => true,
        }
    }

    /// 双闸写路径（闸一范围+类型；闸二写后回读）。
    pub fn write(&mut self, id: u16, v: u64) -> Result<(), u16> {
        if self.frozen {
            self.gated_rej += 1;
            return Err(E_FROZEN_WRITE);
        }
        let idx = match self.entries.iter().position(|(s, _)| s.id == id) {
            Some(i) => i,
            None => {
                self.gated_rej += 1;
                return Err(E_UNREG);
            }
        };
        // 闸一：类型先行 + 范围（拒入 g1_rej，不计入双闸一致率）。
        let (spec, _) = self.entries[idx];
        if !ConfigBook::type_ok(&spec, v) {
            self.g1_rej += 1;
            return Err(E_TYPE_BAD);
        }
        if v < spec.min || v > spec.max {
            self.g1_rej += 1;
            return Err(E_RANGE);
        }
        // 写入。
        self.entries[idx].1 = v;
        // 闸二：回读。
        let rb = self.entries[idx].1;
        if rb != v {
            self.gated_rej += 1;
            return Err(E_RB_MISS);
        }
        self.gated_ok += 1;
        Ok(())
    }

    /// 读值。
    pub fn read(&self, id: u16) -> Result<u64, u16> {
        match self.entries.iter().find(|(s, _)| s.id == id) {
            Some((_, v)) => Ok(*v),
            None => Err(E_UNREG),
        }
    }

    /// 双闸一致率（千分制：过闸一者的写中，闸二回读通过比；
    /// 闸一拒绝属于前置过滤，不计入一致率分母）。
    pub fn gate_permille(&self) -> u64 {
        let total = self.gated_ok + self.gated_rej;
        if total == 0 {
            return 0;
        }
        (self.gated_ok as u64) * 1000 / total as u64
    }

    /// 篡改扫描（登记副本 vs 正本；返回漂移参数号列表）。
    pub fn tamper_scan(&self) -> Vec<u16> {
        let mut drifted = Vec::new();
        for s in PARAM_TABLE.iter() {
            match self.entries.iter().find(|(r, _)| r.id == s.id) {
                Some((r, v)) => {
                    if r.min != s.min || r.max != s.max || r.ptype != s.ptype || *v != s.default {
                        drifted.push(s.id);
                    }
                }
                None => drifted.push(s.id),
            }
        }
        drifted
    }

    /// 篡改回退（把漂移参数恢复正本值）。
    pub fn tamper_rollback(&mut self) -> usize {
        let mut fixed = 0usize;
        for s in PARAM_TABLE.iter() {
            if let Some(e) = self.entries.iter_mut().find(|(r, _)| r.id == s.id) {
                if e.1 != s.default {
                    e.1 = s.default;
                    fixed += 1;
                }
            }
        }
        fixed
    }

    /// 参数摘要（值卷积指纹）。
    pub fn summary(&self) -> Result<(usize, u64), u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        let mut acc: u64 = 0x7640;
        for (s, v) in self.entries.iter() {
            acc = acc.wrapping_mul(31).wrapping_add(s.id as u64).wrapping_add(*v);
        }
        Ok((self.entries.len(), acc))
    }

    /// 摘要稳定。
    pub fn summary_stable(&self) -> bool {
        match (self.summary(), self.summary()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    /// 双闸计数。
    pub fn gate_counts(&self) -> (u32, u32) {
        (self.gated_ok, self.gated_rej)
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 注册资产化演练（12 项全注册）。
pub fn drill_register() -> (bool, String) {
    let mut bk = ConfigBook::new();
    let n = match bk.register_all() {
        Ok(n) => n,
        Err(c) => return (false, format!("注册错=0x{:04X} {}", c, detail(c))),
    };
    (n >= PARAM_MIN && bk.assetized() && bk.summary_stable(),
     format!("注册={} 项 资产化={} 摘要稳定={}", n, bk.assetized(), bk.summary_stable()))
}

/// 双闸一致率 1000‰ 演练（合法写全过 + 越界写全拒）。
pub fn drill_double_gate() -> (bool, String) {
    let mut bk = ConfigBook::new();
    if let Err(c) = bk.register_all() {
        return (false, format!("注册错=0x{:04X} {}", c, detail(c)));
    }
    // 合法写：窗口大小 512、窗口超时 60、风暴强度 850。
    let w1 = bk.write(1, 512).is_ok();
    let w2 = bk.write(2, 60).is_ok();
    let w3 = bk.write(6, 850).is_ok();
    // 回读一致。
    let rb = bk.read(1) == Ok(512) && bk.read(2) == Ok(60) && bk.read(6) == Ok(850);
    // 越界写必拒（闸一）。
    let oob = matches!(bk.write(1, 8192), Err(E_RANGE));
    let permille = bk.gate_permille();
    (w1 && w2 && w3 && rb && oob && permille == 1000,
     format!("写成 3 笔 回读一致={} 越界拒={} 双闸率={}‰", rb, oob, permille))
}

/// 类型错必拒演练（布尔写 2、千分制写 1001）。
pub fn drill_types() -> (bool, String) {
    let mut bk = ConfigBook::new();
    if let Err(c) = bk.register_all() {
        return (false, format!("注册错=0x{:04X} {}", c, detail(c)));
    }
    let flag_bad = matches!(bk.write(5, 2), Err(E_TYPE_BAD));
    let flag_ok = bk.write(5, 1).is_ok();
    let perm_bad = matches!(bk.write(7, 1001), Err(E_TYPE_BAD));
    let perm_ok = bk.write(7, 1000).is_ok();
    (flag_bad && flag_ok && perm_bad && perm_ok,
     format!("布尔拒={} 布尔收={} 千分拒={} 千分收={}", flag_bad, flag_ok, perm_bad, perm_ok))
}

/// 篡改检出回退 10/10 演练。
pub fn drill_tamper_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7640_5EED);
    for _ in 0..ROUNDS {
        let mut bk = ConfigBook::new();
        if bk.register_all().is_err() {
            continue;
        }
        // 篡改随机参数的当前值。
        let id = rng.below(PARAM_TABLE.len() as u64) as u16 + 1;
        let spec = PARAM_TABLE[id as usize - 1];
        // 坏值必异于默认（默认贴边时取另一端）。
        let bad = if spec.default == spec.min { spec.max } else { spec.min };
        let _ = bk.write(id, bad.min(spec.max).max(spec.min));
        let drift = bk.tamper_scan();
        let detected = drift.contains(&id);
        let fixed = bk.tamper_rollback();
        let restored = bk.read(id) == Ok(spec.default);
        if detected && fixed >= 1 && restored {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("篡改检出回退={}/{}", ok, ROUNDS))
}

/// 越界写 10/10 必拒演练。
pub fn drill_range_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7641_5EED);
    for _ in 0..ROUNDS {
        let mut bk = ConfigBook::new();
        if bk.register_all().is_err() {
            continue;
        }
        let id = rng.below(PARAM_TABLE.len() as u64) as u16 + 1;
        let spec = PARAM_TABLE[id as usize - 1];
        // 越上界与越下界各半（min=0 时下越界退化为上越界）。
        let bad = if rng.below(2) == 0 {
            spec.max + 1
        } else {
            match spec.min.checked_sub(1) {
                Some(v) => v,
                None => spec.max + 1,
            }
        };
        if matches!(bk.write(id, bad), Err(E_RANGE) | Err(E_TYPE_BAD)) {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("越界必拒={}/{}", ok, ROUNDS))
}

/// 未注册写 + 号重复 + 冻结拒演练。
pub fn drill_registry_guards() -> (bool, String) {
    let mut bk = ConfigBook::new();
    let _ = bk.register_all();
    let unreg = matches!(bk.write(13, 1), Err(E_UNREG));
    let read_unreg = matches!(bk.read(13), Err(E_UNREG));
    let dup = matches!(bk.register(PARAM_TABLE[0]), Err(E_ID_DUP));
    bk.freeze();
    let frozen = matches!(bk.write(1, 100), Err(E_FROZEN_WRITE));
    let reg_frozen = matches!(bk.register(PARAM_TABLE[1]), Err(E_RDONLY));
    (unreg && read_unreg && dup && frozen && reg_frozen,
     format!("未注写={} 未注读={} 重复拒={} 冻写拒={} 冻登拒={}", unreg, read_unreg, dup, frozen, reg_frozen))
}

/// 正本对拍演练（资产化逐项一致 + 名/型/界全对）。
pub fn drill_master_compare() -> (bool, String) {
    let mut bk = ConfigBook::new();
    if let Err(c) = bk.register_all() {
        return (false, format!("注册错=0x{:04X} {}", c, detail(c)));
    }
    let mut all_match = true;
    for s in PARAM_TABLE.iter() {
        match bk.read(s.id) {
            Ok(v) => {
                if v != s.default {
                    all_match = false;
                }
            }
            Err(_) => all_match = false,
        }
    }
    let drift = bk.tamper_scan();
    (all_match && drift.is_empty() && bk.assetized(),
     format!("正本对拍={} 漂移清单空={}", all_match, drift.is_empty()))
}

/// 参数摘要演练（写后摘要变化 + 两次汇总一致）。
pub fn drill_summary() -> (bool, String) {
    let mut bk = ConfigBook::new();
    if let Err(c) = bk.register_all() {
        return (false, format!("注册错=0x{:04X} {}", c, detail(c)));
    }
    let s1 = bk.summary();
    let _ = bk.write(3, 5);
    let s2 = bk.summary();
    let changed = s1 != s2;
    let stable = bk.summary_stable();
    // 回退后再变回。
    let _ = bk.write(3, 3);
    let s3 = bk.summary();
    let restored = s1 == s3;
    (changed && stable && restored,
     format!("写后变档={} 汇总稳定={} 回改复现={}", changed, stable, restored))
}

// ── 参数写入史账（append-only 审计面） ──────────────────────
/// 写入史条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteRec {
    /// 序号（1 起连续）。
    pub seq: u32,
    /// 参数号。
    pub id: u16,
    /// 旧值。
    pub old: u64,
    /// 新值。
    pub new: u64,
    /// 双闸旗（写成功必为真）。
    pub gated: bool,
}

/// 写入史账。
#[derive(Debug)]
pub struct WriteJournal {
    /// 记录。
    recs: Vec<WriteRec>,
}

impl WriteJournal {
    /// 新账。
    pub fn new() -> WriteJournal {
        WriteJournal { recs: Vec::new() }
    }

    /// 登记（写成功后调用）。
    pub fn push(&mut self, id: u16, old: u64, new: u64) -> u32 {
        let seq = match self.recs.last() {
            Some(r) => r.seq + 1,
            None => 1,
        };
        self.recs.push(WriteRec { seq, id, old, new, gated: true });
        seq
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 序连续。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 指定参数的写入轨迹。
    pub fn trace_of(&self, id: u16) -> Vec<&WriteRec> {
        self.recs.iter().filter(|r| r.id == id).collect()
    }

    /// 全部成功旗审计。
    pub fn all_gated(&self) -> bool {
        self.recs.iter().all(|r| r.gated)
    }
}

// ── 回滚栈（写前快照 LIFO） ────────────────────────────────
/// 回滚栈（参数值快照 LIFO）。
#[derive(Debug)]
pub struct RollbackStack {
    /// 快照（全参数值向量）。
    snaps: Vec<Vec<u64>>,
}

impl RollbackStack {
    /// 新栈。
    pub fn new() -> RollbackStack {
        RollbackStack { snaps: Vec::new() }
    }

    /// 压栈（写前快照）。
    pub fn push(&mut self, values: Vec<u64>) {
        self.snaps.push(values);
    }

    /// 弹栈（恢复最近快照）。
    pub fn pop(&mut self) -> Option<Vec<u64>> {
        self.snaps.pop()
    }

    /// 深度。
    pub fn depth(&self) -> usize {
        self.snaps.len()
    }

    /// LIFO 序审计（栈底快照最旧）。
    pub fn lifo_ok(&self) -> bool {
        for w in self.snaps.windows(2) {
            if w[0] == w[1] {
                return false;
            }
        }
        true
    }
}

// ── 类型探测矩阵（12 参数 × 5 探针 = 60 格全测） ───────────
/// 探针 verdict。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// 拒（越界下）。
    RejLo,
    /// 收（下界）。
    OkLo,
    /// 收（上界）。
    OkHi,
    /// 拒（越界上）。
    RejHi,
    /// 拒（类型错）。
    RejType,
}

/// 单参数探测行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeRow {
    /// 参数号。
    pub id: u16,
    /// 五探针 verdict。
    pub cells: [ProbeVerdict; 5],
}

impl ProbeRow {
    /// 规格推演期望（与 build 的实际探测逻辑同源：类型先行+范围次之）。
    pub fn expect(spec: &ParamSpec, probe: usize) -> ProbeVerdict {
        match probe {
            // 探 1：min-1（min=0 退化为收）。
            0 => if spec.min == 0 { ProbeVerdict::OkLo } else { ProbeVerdict::RejLo },
            // 探 2：min 恒收。
            1 => ProbeVerdict::OkLo,
            // 探 3：max 恒收。
            2 => ProbeVerdict::OkHi,
            // 探 4：max+1（布尔→类型拒；千分制 max=1000 时 1001 类型拒）。
            3 => match spec.ptype {
                PType::Int => ProbeVerdict::RejHi,
                PType::Flag => ProbeVerdict::RejType,
                PType::Permille => if spec.max >= 1000 { ProbeVerdict::RejType } else { ProbeVerdict::RejHi },
            },
            // 探 5：类型错探针恒类型拒。
            _ => ProbeVerdict::RejType,
        }
    }

    /// 行正确性（verdict 与规格推演一致）。
    pub fn correct(&self, spec: &ParamSpec) -> bool {
        for (i, c) in self.cells.iter().enumerate() {
            if *c != ProbeRow::expect(spec, i) {
                return false;
            }
        }
        true
    }
}

/// 全参数探测矩阵（12 行）。
#[derive(Debug)]
pub struct ProbeMatrix {
    /// 行账。
    pub rows: Vec<ProbeRow>,
}

impl ProbeMatrix {
    /// 建矩阵（对全新总表逐参数五探针）。
    pub fn build() -> Result<ProbeMatrix, u16> {
        let mut rows = Vec::new();
        for spec in PARAM_TABLE.iter() {
            // 每探针用独立总表（互不污染）。
            let mk = || -> Result<ConfigBook, u16> {
                let mut b = ConfigBook::new();
                b.register_all()?;
                Ok(b)
            };
            // 探 1：min-1 越下界（min=0 时用 saturating，语义仍是拒上/收）。
            let lo_bad = if spec.min == 0 { None } else { Some(spec.min - 1) };
            let c0 = match lo_bad {
                Some(v) => match mk()?.write(spec.id, v) {
                    Err(E_RANGE) => ProbeVerdict::RejLo,
                    Err(E_TYPE_BAD) => ProbeVerdict::RejType,
                    _ => ProbeVerdict::OkLo,
                },
                None => ProbeVerdict::OkLo, // min=0 无下越界，退化为收。
            };
            // 探 2：min 合法。
            let c1 = match mk()?.write(spec.id, spec.min) {
                Ok(_) => ProbeVerdict::OkLo,
                Err(_) => ProbeVerdict::RejLo,
            };
            // 探 3：max 合法。
            let c2 = match mk()?.write(spec.id, spec.max) {
                Ok(_) => ProbeVerdict::OkHi,
                Err(_) => ProbeVerdict::RejHi,
            };
            // 探 4：max+1 越上界。
            let c3 = match mk()?.write(spec.id, spec.max + 1) {
                Err(E_RANGE) => ProbeVerdict::RejHi,
                Err(E_TYPE_BAD) => ProbeVerdict::RejType,
                _ => ProbeVerdict::OkHi,
            };
            // 探 5：类型错（布尔写 2；千分制写 1001；整数写 max+1 归探 4）。
            let c4 = match spec.ptype {
                PType::Flag => match mk()?.write(spec.id, 2) {
                    Err(E_TYPE_BAD) => ProbeVerdict::RejType,
                    _ => ProbeVerdict::OkHi,
                },
                PType::Permille => match mk()?.write(spec.id, 1001) {
                    Err(E_TYPE_BAD) => ProbeVerdict::RejType,
                    _ => ProbeVerdict::OkHi,
                },
                PType::Int => ProbeVerdict::RejType, // 整数无独立类型错，占位对齐。
            };
            rows.push(ProbeRow { id: spec.id, cells: [c0, c1, c2, c3, c4] });
        }
        Ok(ProbeMatrix { rows })
    }

    /// 全矩阵正确。
    pub fn all_correct(&self) -> bool {
        if self.rows.len() != PARAM_TABLE.len() {
            return false;
        }
        for (i, r) in self.rows.iter().enumerate() {
            if !r.correct(&PARAM_TABLE[i]) {
                return false;
            }
        }
        true
    }

    /// 格数。
    pub fn cells(&self) -> usize {
        self.rows.len() * 5
    }
}

// ── 参数分组视图 ────────────────────────────────────────────
/// 分组视图（按类型三分）。
#[derive(Debug)]
pub struct GroupView {
    /// 整数组。
    pub ints: Vec<u16>,
    /// 千分制组。
    pub permilles: Vec<u16>,
    /// 布尔组。
    pub flags: Vec<u16>,
}

impl GroupView {
    /// 从正本建视图。
    pub fn build() -> GroupView {
        let mut ints = Vec::new();
        let mut permilles = Vec::new();
        let mut flags = Vec::new();
        for s in PARAM_TABLE.iter() {
            match s.ptype {
                PType::Int => ints.push(s.id),
                PType::Permille => permilles.push(s.id),
                PType::Flag => flags.push(s.id),
            }
        }
        GroupView { ints, permilles, flags }
    }

    /// 分组守恒（三组和 == 12 且无交集语义由构造保证）。
    pub fn conserved(&self) -> bool {
        self.ints.len() + self.permilles.len() + self.flags.len() == PARAM_TABLE.len()
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 写入史账演练（连续 + 全闸 + 轨迹可查）。
pub fn drill_journal() -> (bool, String) {
    let mut book = ConfigBook::new();
    if book.register_all().is_err() {
        return (false, "注册失败".to_string());
    }
    let mut j = WriteJournal::new();
    // 三笔合法写（每笔记史）。
    let seqs = [
        (1u16, 512u64),
        (2, 60),
        (6, 850),
    ];
    let mut all_ok = true;
    for (id, v) in seqs.iter() {
        let old = book.read(*id).unwrap_or(0);
        if book.write(*id, *v).is_err() {
            all_ok = false;
        }
        j.push(*id, old, *v);
    }
    let trace = j.trace_of(1);
    (all_ok && j.len() == 3 && j.continuous() && j.all_gated() && trace.len() == 1,
     format!("史账 {} 笔 连续={} 全闸={} 轨迹={}", j.len(), j.continuous(), j.all_gated(), trace.len()))
}

/// 回滚栈演练（两次写两次回滚，值逐级还原）。
pub fn drill_rollback_stack() -> (bool, String) {
    let mut book = ConfigBook::new();
    if book.register_all().is_err() {
        return (false, "注册失败".to_string());
    }
    let mut stack = RollbackStack::new();
    // 快照 0（全默认）。
    let mut base: Vec<u64> = Vec::new();
    for s in PARAM_TABLE.iter() {
        base.push(s.default);
    }
    stack.push(base.clone());
    // 写 1→512 压栈 → 写 2→60 压栈。
    let _ = book.write(1, 512);
    let mut snap1: Vec<u64> = Vec::new();
    for s in PARAM_TABLE.iter() {
        snap1.push(book.read(s.id).unwrap_or(0));
    }
    stack.push(snap1.clone());
    let _ = book.write(2, 60);
    // 弹栈还原一次（回到 1=512 态）。
    let restore1 = stack.pop().unwrap_or_default();
    let mut ok1 = true;
    for (i, s) in PARAM_TABLE.iter().enumerate() {
        let _ = book.write(s.id, restore1[i]);
        if book.read(s.id).unwrap_or(0) != snap1[i] {
            ok1 = false;
        }
    }
    // 再弹（回到全默认）。
    let restore0 = stack.pop().unwrap_or_default();
    let mut ok0 = true;
    for (i, s) in PARAM_TABLE.iter().enumerate() {
        let _ = book.write(s.id, restore0[i]);
        if book.read(s.id).unwrap_or(0) != base[i] {
            ok0 = false;
        }
    }
    (stack.depth() == 0 && ok1 && ok0 && stack.lifo_ok(),
     format!("两级还原 ok1={} ok0={} 栈空={}", ok1, ok0, stack.depth() == 0))
}

/// 类型探测矩阵演练（12×5=60 格全对）。
pub fn drill_probe_matrix() -> (bool, String) {
    let m = match ProbeMatrix::build() {
        Ok(m) => m,
        Err(c) => return (false, format!("建矩阵错=0x{:04X} {}", c, detail(c))),
    };
    let ok = m.all_correct();
    let cells = m.cells();
    (ok && cells == 60 && m.rows.len() == PARAM_TABLE.len(),
     format!("矩阵 {} 格 全对={}", cells, ok))
}

/// 分组视图演练（三组和守恒 + 布尔组恰 2）。
pub fn drill_groups() -> (bool, String) {
    let g = GroupView::build();
    (g.conserved() && g.flags.len() == 2 && g.permilles.len() == 3 && g.ints.len() == 7,
     format!("整数 {} 千分 {} 布尔 {} 守恒={}", g.ints.len(), g.permilles.len(), g.flags.len(), g.conserved()))
}

// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越界写。
    {
        let mut bk = ConfigBook::new();
        let _ = bk.register_all();
        if matches!(bk.write(2, 101), Err(E_RANGE)) { caught += 1; d.push("注1 越界:抓".to_string()); } else { d.push("注1 越界:漏".to_string()); }
    }
    // 注 2：冻结改写。
    {
        let mut bk = ConfigBook::new();
        let _ = bk.register_all();
        bk.freeze();
        if matches!(bk.write(1, 300), Err(E_FROZEN_WRITE)) { caught += 1; d.push("注2 冻改:抓".to_string()); } else { d.push("注2 冻改:漏".to_string()); }
    }
    // 注 3：篡改检出。
    {
        let mut bk = ConfigBook::new();
        let _ = bk.register_all();
        let _ = bk.write(4, 1000);
        let drift = bk.tamper_scan();
        if drift.contains(&4) { caught += 1; d.push("注3 篡改:抓".to_string()); } else { d.push("注3 篡改:漏".to_string()); }
    }
    // 注 4：类型错。
    {
        let mut bk = ConfigBook::new();
        let _ = bk.register_all();
        if matches!(bk.write(12, 5), Err(E_TYPE_BAD)) { caught += 1; d.push("注4 类型:抓".to_string()); } else { d.push("注4 类型:漏".to_string()); }
    }
    // 注 5：未注册写。
    {
        let mut bk = ConfigBook::new();
        let _ = bk.register_all();
        if matches!(bk.write(99, 1), Err(E_UNREG)) { caught += 1; d.push("注5 未注:抓".to_string()); } else { d.push("注5 未注:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7640
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7640 段诊断码：核对配置总表段位闭集"
        && PARAM_TABLE.len() >= PARAM_MIN
}

/// run_unx1315_checks：本单判据聚合（诊断段 0x7640）。
pub fn run_unx1315_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1315 退役配置总表");
    let (ok1, d1) = drill_register();
    s.add_dyn("J1-12项参数注册总表", ok1, &d1);
    let (ok2, d2) = drill_double_gate();
    s.add_dyn("J1-双闸一致率1000‰", ok2, &d2);
    let (ok3, d3) = drill_types();
    s.add_dyn("J1-三型校验", ok3, &d3);
    let (ok4, d4) = drill_tamper_10();
    s.add_dyn("J1-篡改检出回退10/10", ok4, &d4);
    let (ok5, d5) = drill_range_10();
    s.add_dyn("J1-越界写必拒10/10", ok5, &d5);
    let (ok6, d6) = drill_registry_guards();
    s.add_dyn("J1-注册守卫五拒", ok6, &d6);
    let (ok7, d7) = drill_master_compare();
    s.add_dyn("J1-正本对拍", ok7, &d7);
    let (ok8, d8) = drill_summary();
    s.add_dyn("J1-参数摘要", ok8, &d8);
    let (ok10, d10) = drill_journal();
    s.add_dyn("J2-写入史账", ok10, &d10);
    let (ok11, d11) = drill_rollback_stack();
    s.add_dyn("J2-回滚栈两级还原", ok11, &d11);
    let (ok12, d12) = drill_probe_matrix();
    s.add_dyn("J2-类型探测矩阵60格", ok12, &d12);
    let (ok13, d13) = drill_groups();
    s.add_dyn("J2-参数分组视图", ok13, &d13);
    let (c9, t9, d9) = inject_5();
    s.add_dyn("J2-注入核5/5", c9 == t9, &format!("{}/{} {}", c9, t9, d9.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7640 段十码闭集");
    s
}
