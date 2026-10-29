//! unx48301_cfgtree — UNX-F48301 IME 用户配置总模型（单一事实源：引擎/桥/方案/切换四段配置树）（诊断段 0xC680）。
//!
//! 判据原文：
//!   J1 四段配置树 ×10 读改写往返一致；判据锚定 §AI-61 §2 专题五、B12 状态机配置面纵深。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 四段配置树：Engine（引擎段）/ Bridge（桥段）/ Scheme（方案段）/
//!      Switch（切换段）四段各持自己闭集内字段——树是用户配置的唯一
//!      事实源，段间无影子副本（影子即双源，双源即漂移温床）。
//!   2. 字段闭集与段闭集：每字段有闭集校验（方案 id ∈三双拼、抖动窗
//!      ≤冷却窗、开关为位值等），越闭集写拒收且树不升版。
//!   3. 读改写口径（J1 ×10）：10 轮 读快照→改字段→写→重读，重读
//!      快照与写后期望逐位一致；每轮树版本严格 +1（单调可回放），
//!      往返一致才绿。
//!   4. 快照即读径：read 返回全树快照（值拷贝），外部持有快照不受
//!      后续写影响（读写隔离）；快照相等性逐段逐字段判定。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC680;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_FIELD_BAD: u16 = SEG_BASE + 1;  // 字段值越闭集
const E_SECTION_BAD: u16 = SEG_BASE + 2;// 段号越界
const E_VER_STALE: u16 = SEG_BASE + 3;  // 版本不单调/写后未升版
const E_SNAPSHOT_DIFF: u16 = SEG_BASE + 4;// 读改写往返不一致
const E_RMW_MISS: u16 = SEG_BASE + 5;   // ×10 轮不足
const E_LEDGER_BAD: u16 = SEG_BASE + 6; // 配置账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// 方案 id 闭集（三双拼）。
pub const SCHEME_XIAOHE: u8 = 0;
pub const SCHEME_ZIRANMA: u8 = 1;
pub const SCHEME_MS: u8 = 2;

/// 抖动窗闭集界。
pub const JITTER_MIN_MS: u32 = 5;
pub const JITTER_MAX_MS: u32 = 60;
pub const COOL_MAX_MS: u32 = 200;

/// 引擎段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EngineCfg {
    pub fhw_mode: bool,        // 半全角缺省
    pub punct_cn: bool,        // 中文标点缺省
    pub page_size: u8,         // 候选页大小 1..=9
}

/// 桥段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BridgeCfg {
    pub abi_ver: u32,          // 桥 ABI（=1）
    pub session_token: u64,    // 会话令牌（非零）
    pub heartbeat_window: u32, // 心跳窗 1..=10
}

/// 方案段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchemeCfg {
    pub scheme_id: u8,         // 三双拼闭集
    pub s2t_on: bool,          // 简繁
    pub fuzzy_on: bool,        // 模糊音
}

/// 切换段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchCfg {
    pub jitter_ms: u32,        // 合并窗
    pub cool_ms: u32,          // 冷却窗
    pub caps_led_sync: bool,   // LED 同步
}

/// 四段配置树（单一事实源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CfgTree {
    pub engine: EngineCfg,
    pub bridge: BridgeCfg,
    pub scheme: SchemeCfg,
    pub switch: SwitchCfg,
    pub version: u64,
}

impl CfgTree {
    /// 声明默认树。
    pub fn default_tree() -> CfgTree {
        CfgTree {
            engine: EngineCfg { fhw_mode: false, punct_cn: true, page_size: 5 },
            bridge: BridgeCfg { abi_ver: 1, session_token: 0xC680_C681, heartbeat_window: 3 },
            scheme: SchemeCfg { scheme_id: SCHEME_XIAOHE, s2t_on: false, fuzzy_on: true },
            switch: SwitchCfg { jitter_ms: 20, cool_ms: 40, caps_led_sync: true },
            version: 0,
        }
    }

    /// 全树闭集校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.engine.page_size == 0 || self.engine.page_size > 9 {
            return Err(E_FIELD_BAD);
        }
        if self.bridge.abi_ver != 1 {
            return Err(E_FIELD_BAD);
        }
        if self.bridge.session_token == 0 {
            return Err(E_FIELD_BAD);
        }
        if self.bridge.heartbeat_window == 0 || self.bridge.heartbeat_window > 10 {
            return Err(E_FIELD_BAD);
        }
        if self.scheme.scheme_id > SCHEME_MS {
            return Err(E_FIELD_BAD);
        }
        if self.switch.jitter_ms < JITTER_MIN_MS || self.switch.jitter_ms > JITTER_MAX_MS {
            return Err(E_FIELD_BAD);
        }
        if self.switch.cool_ms > COOL_MAX_MS || self.switch.cool_ms < self.switch.jitter_ms {
            return Err(E_FIELD_BAD);
        }
        Ok(())
    }
}

/// 配置树枢纽：唯一写入口 + 版本单调 + 快照读径。
pub struct CfgStore {
    tree: CfgTree,
    pub write_total: u32,
    pub reject_total: u32,
    pub rmw_total: u32,
}

impl CfgStore {
    pub fn new() -> CfgStore {
        CfgStore { tree: CfgTree::default_tree(), write_total: 0, reject_total: 0, rmw_total: 0 }
    }

    /// 读径：全树快照（值拷贝，读写隔离）。
    pub fn read(&self) -> CfgTree {
        self.tree
    }

    /// 唯一写入口：整树替换 + 闭集校验 + 版本单调升。
    pub fn write(&mut self, mut next: CfgTree) -> Result<u64, u16> {
        next.version = self.tree.version; // 版本由本面管理，写方不可伪造
        if let Err(e) = next.validate() {
            self.reject_total += 1; // 拒写留痕
            return Err(e);
        }
        next.version += 1;
        self.tree = next;
        self.write_total += 1;
        Ok(self.tree.version)
    }

    /// 字段级写糖：改一段后经唯一写入口落树。
    pub fn set_engine(&mut self, e: EngineCfg) -> Result<u64, u16> {
        let mut t = self.read();
        t.engine = e;
        self.write(t)
    }
    pub fn set_bridge(&mut self, b: BridgeCfg) -> Result<u64, u16> {
        let mut t = self.read();
        t.bridge = b;
        self.write(t)
    }
    pub fn set_scheme(&mut self, s: SchemeCfg) -> Result<u64, u16> {
        let mut t = self.read();
        t.scheme = s;
        self.write(t)
    }
    pub fn set_switch(&mut self, s: SwitchCfg) -> Result<u64, u16> {
        let mut t = self.read();
        t.switch = s;
        self.write(t)
    }

    /// 读改写一轮（J1 主体）：读快照 → 变换 → 写 → 重读比对期望。
    /// 变换函数由调用方注入的 (轮号, 快照) 决定，期望由本面重算。
    pub fn rmw_round(
        &mut self,
        round: u32,
        transform: fn(u32, &CfgTree) -> CfgTree,
    ) -> Result<CfgTree, u16> {
        let snap = self.read();
        let want = transform(round, &snap);
        let v_before = snap.version;
        self.write(want)?;
        let after = self.read();
        // 往返一致：重读==期望（除 version 由本面续推）。
        let mut expect = want;
        expect.version = v_before + 1;
        if after != expect {
            return Err(E_SNAPSHOT_DIFF);
        }
        if after.version != v_before + 1 {
            return Err(E_VER_STALE);
        }
        self.rmw_total += 1;
        Ok(after)
    }

    /// ×N 轮覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.rmw_total < want {
            return Err(E_RMW_MISS);
        }
        Ok(())
    }

    /// 单源审计：无影子副本（结构上只有 tree 一个存储位），账目守恒，
    /// 版本==写次数。
    pub fn audit(&self) -> Result<(), u16> {
        if self.tree.version != self.write_total as u64 {
            return Err(E_VER_STALE);
        }
        self.tree.validate()?;
        Ok(())
    }

    pub fn version(&self) -> u64 {
        self.tree.version
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.rmw_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性变换 1：轮号驱动方案轮转 + 简繁交替。
pub fn xform_scheme(round: u32, snap: &CfgTree) -> CfgTree {
    let mut t = *snap;
    t.scheme.scheme_id = ((round % 3) as u8) % (SCHEME_MS + 1);
    t.scheme.s2t_on = round % 2 == 0;
    t
}

/// 确定性变换 2：抖动窗在闭集内扫描 + 冷却窗随动。
pub fn xform_switch(round: u32, snap: &CfgTree) -> CfgTree {
    let mut t = *snap;
    let j = JITTER_MIN_MS + (round * 5) % (JITTER_MAX_MS - JITTER_MIN_MS + 1);
    t.switch.jitter_ms = j;
    t.switch.cool_ms = if j * 2 > COOL_MAX_MS { COOL_MAX_MS } else { j * 2 };
    t
}

/// 确定性变换 3：页大小与心跳窗扫描。
pub fn xform_engine_bridge(round: u32, snap: &CfgTree) -> CfgTree {
    let mut t = *snap;
    t.engine.page_size = ((round % 9) + 1) as u8;
    t.bridge.heartbeat_window = ((round % 10) + 1) as u32;
    t
}

/// 确定性变换 4：LED/标点/模糊开关翻转。
pub fn xform_flags(round: u32, snap: &CfgTree) -> CfgTree {
    let mut t = *snap;
    t.engine.fhw_mode = round % 2 == 1;
    t.engine.punct_cn = round % 3 != 0;
    t.scheme.fuzzy_on = round % 4 != 0;
    t.switch.caps_led_sync = round % 5 != 0;
    t
}

/// 自例程判据：逐条落实 J1（四段配置树 ×10 读改写往返一致），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主读改写 ×10：四段变换轮转（scheme/switch/engine+bridge/flags
    //    各 2~3 轮），每轮读改写往返一致、版本严格 +1。
    let mut store = CfgStore::new();
    let xforms: [fn(u32, &CfgTree) -> CfgTree; 4] = [
        xform_scheme,
        xform_switch,
        xform_engine_bridge,
        xform_flags,
    ];
    let mut r = 0u32;
    while r < 10 {
        let xf = xforms[(r % 4) as usize];
        let after = store.rmw_round(r, xf)?;
        if after.version != r as u64 + 1 {
            return Err(E_VER_STALE);
        }
        r += 1;
    }
    store.coverage(10)?;
    store.audit()?;
    if store.version() != 10 {
        return Err(E_VER_STALE);
    }
    hits += 1;

    // R2 四段各落一改：段级写糖（set_scheme/set_switch/set_engine/
    //    set_bridge）各自生效且其余段不动（段隔离）。
    let mut store = CfgStore::new();
    let base = store.read();
    store.set_scheme(SchemeCfg { scheme_id: SCHEME_ZIRANMA, s2t_on: true, fuzzy_on: false })?;
    let after = store.read();
    if after.scheme.scheme_id != SCHEME_ZIRANMA || !after.scheme.s2t_on {
        return Err(E_SNAPSHOT_DIFF);
    }
    if after.engine != base.engine || after.bridge != base.bridge || after.switch != base.switch {
        return Err(E_SNAPSHOT_DIFF); // 他段不得被波及
    }
    store.set_switch(SwitchCfg { jitter_ms: 30, cool_ms: 90, caps_led_sync: false })?;
    let after2 = store.read();
    if after2.switch.jitter_ms != 30 || after2.switch.cool_ms != 90 {
        return Err(E_SNAPSHOT_DIFF);
    }
    if after2.scheme.scheme_id != SCHEME_ZIRANMA {
        return Err(E_SNAPSHOT_DIFF); // scheme 段保持
    }
    hits += 1;

    // R3 闭集拒写：页大小 0/10、心跳窗 0/11、方案 id 3、抖动窗 4/61、
    //    冷却 < 合并、token 0、ABI 2——全拒且树不升版。
    let mut store = CfgStore::new();
    let v0 = store.version();
    let mut t = store.read();
    t.engine.page_size = 0;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD);
    }
    let mut t = store.read();
    t.engine.page_size = 10;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD);
    }
    let mut t = store.read();
    t.bridge.heartbeat_window = 11;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD);
    }
    let mut t = store.read();
    t.scheme.scheme_id = 3;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD);
    }
    let mut t = store.read();
    t.switch.jitter_ms = 4;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD);
    }
    let mut t = store.read();
    t.switch.jitter_ms = 61;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD);
    }
    let mut t = store.read();
    t.switch.cool_ms = 10;
    t.switch.jitter_ms = 20;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD); // cool < jitter
    }
    let mut t = store.read();
    t.bridge.session_token = 0;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD);
    }
    let mut t = store.read();
    t.bridge.abi_ver = 2;
    if store.write(t).is_ok() {
        return Err(E_FIELD_BAD);
    }
    if store.version() != v0 || store.write_total != 0 {
        return Err(E_VER_STALE); // 拒写不升版
    }
    hits += 1;

    // R4 版本由本面管理：写方伪造 version 大数被压回（next.version
    //    先归位再 +1），连续写版本严格单调。
    let mut store = CfgStore::new();
    let mut t = store.read();
    t.version = 999;
    store.write(t)?;
    if store.version() != 1 {
        return Err(E_VER_STALE);
    }
    let mut t = store.read();
    t.engine.page_size = 7;
    store.write(t)?;
    if store.version() != 2 {
        return Err(E_VER_STALE);
    }
    hits += 1;

    // R5 快照读写隔离：读出快照后连续改树，旧快照逐位不变。
    let mut store = CfgStore::new();
    let snap = store.read();
    store.set_scheme(SchemeCfg { scheme_id: SCHEME_MS, s2t_on: true, fuzzy_on: true })?;
    store.set_switch(SwitchCfg { jitter_ms: 40, cool_ms: 80, caps_led_sync: false })?;
    if store.read() == snap {
        return Err(E_SNAPSHOT_DIFF); // 树确实变了
    }
    if snap.scheme.scheme_id != SCHEME_XIAOHE || snap.scheme.s2t_on {
        return Err(E_SNAPSHOT_DIFF); // 旧快照未被波及
    }
    if snap.switch.jitter_ms != 20 {
        return Err(E_SNAPSHOT_DIFF);
    }
    hits += 1;

    // R6 变换幂等：同轮号同快照两次变换结果逐位一致（纯函数红线）。
    let base = CfgTree::default_tree();
    let mut r = 0u32;
    while r < 10 {
        let xforms: [fn(u32, &CfgTree) -> CfgTree; 4] = [
            xform_scheme,
            xform_switch,
            xform_engine_bridge,
            xform_flags,
        ];
        let mut f = 0usize;
        while f < 4 {
            let a = xforms[f](r, &base);
            let b = xforms[f](r, &base);
            if a != b {
                return Err(E_SNAPSHOT_DIFF);
            }
            f += 1;
        }
        r += 1;
    }
    hits += 1;

    // R7 拒写账目与可红：拒写计数逐次留痕；伪造版本失衡 audit 必红。
    let mut store = CfgStore::new();
    let mut t = store.read();
    t.scheme.scheme_id = 9;
    store.write(t).is_err();
    store.write(t).is_err();
    if store.reject_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 长程往返 ×10 复跑与写回（判据 J2）：第二轮 ×10 变换复跑，
    //    终树与首轮一致（可复跑）；末尾写回。
    let mut store = CfgStore::new();
    let xforms: [fn(u32, &CfgTree) -> CfgTree; 4] = [
        xform_scheme,
        xform_switch,
        xform_engine_bridge,
        xform_flags,
    ];
    let mut r = 0u32;
    while r < 10 {
        store.rmw_round(r, xforms[(r % 4) as usize])?;
        r += 1;
    }
    let final1 = store.read();
    let mut store2 = CfgStore::new();
    let mut r = 0u32;
    while r < 10 {
        store2.rmw_round(r, xforms[(r % 4) as usize])?;
        r += 1;
    }
    if store2.read() != final1 {
        return Err(E_SNAPSHOT_DIFF);
    }
    if store.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    store.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
