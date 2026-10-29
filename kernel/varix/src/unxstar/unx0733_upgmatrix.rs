//! UNX-F0733 · 升级演练矩阵（模拟升级注入） · 诊断段 0x5b20
//! 判据原文摘录：UNX-F0733-J1 演练矩阵过（三场景注入：正常升级/坏包/协议跳变
//! ——三场景全链走通 3/3）；完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 演练矩阵只认三张固定场景卡（正常升级/坏包/协议跳变），注入器按卡面
//!    在全链指定锚点投毒或放行，不发明第四种场景。
//! 2. 全链五站固定：拉取→校验→落台→激活→回执；每站产出可落账的站级结论，
//!    场景全链走通 = 五站全部产出结论且终态与场景预期一致。
//! 3. 注入必须是确定性注入：毒哈希/跳变协议号在构建演练账时写死，
//!    不依赖时钟、随机数或外部状态，复跑结果恒等。
//! 4. 定容数组账本 + 守恒恒等式：pass+fail+abort == executed <= total 场景数。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5b20 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5b20;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_SCEN_DUP: u16 = SEG_BASE + 1; // 场景卡重复登记
const E_SCEN_FULL: u16 = SEG_BASE + 2; // 场景矩阵容量已满
const E_SCEN_NOTFOUND: u16 = SEG_BASE + 3; // 场景卡编号不存在
const E_PKG_EMPTY: u16 = SEG_BASE + 4; // 升级包载荷为空
const E_PKG_HASH: u16 = SEG_BASE + 5; // 包哈希不匹配（坏包注入检出）
const E_PROTO_JUMP: u16 = SEG_BASE + 6; // 协议号跳变（协议跳变注入检出）
const E_PHASE: u16 = SEG_BASE + 7; // 演练状态机非法边
const E_STATION_SKIP: u16 = SEG_BASE + 8; // 全链跳站（禁止跨站执行）
const E_CHAIN_INCOMPLETE: u16 = SEG_BASE + 9; // 全链未走通（站结论缺失）
const E_EXPECT: u16 = SEG_BASE + 10; // 终态与场景预期不一致
const E_BAL: u16 = SEG_BASE + 11; // 守恒恒等式破裂（三态计数不守恒）
const E_SEALED: u16 = SEG_BASE + 12; // 矩阵已封账，拒绝再写
const E_PKG_TOO_BIG: u16 = SEG_BASE + 13; // 包载荷超上限
const E_NO_SCEN: u16 = SEG_BASE + 14; // 矩阵空，无场景可演练

/// 场景矩阵定容：三张固定卡，预留到 8 槽防误扩。
pub const MAX_SCEN: usize = 8;

/// 升级包载荷上限（字节，演练用小包，写死防失控）。
pub const PKG_MAX_BYTES: usize = 4096;

/// 全链五站编号：拉取/校验/落台/激活/回执。
pub const ST_FETCH: u8 = 1;
pub const ST_VERIFY: u8 = 2;
pub const ST_STAGE: u8 = 3;
pub const ST_ACTIVATE: u8 = 4;
pub const ST_REPORT: u8 = 5;

/// 基准协议号（当前引导器与升级工具约定的协议版本）。
pub const PROTO_BASE: u16 = 7;

// ── FNV-1a 64（包完整性唯一口径）───────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SCEN_DUP => "场景卡重复：三张固定卡各演一次，重复登记即矩阵污染",
        E_SCEN_FULL => "矩阵已满 8 槽：固定三卡之外的注入一律拒绝",
        E_SCEN_NOTFOUND => "场景卡不存在：先发卡再演练",
        E_PKG_EMPTY => "升级包为空：空包在拉取站即拦，不进校验",
        E_PKG_HASH => "包哈希不匹配：坏包注入被校验站检出，属预期拦截",
        E_PROTO_JUMP => "协议号跳变：激活站检出协议不兼容，属预期拦截",
        E_PHASE => "演练状态机非法边：先跑完当前站再进下一站",
        E_STATION_SKIP => "跳站：五站必须逐站执行，禁止跨站",
        E_CHAIN_INCOMPLETE => "全链未走通：五站结论必须齐备才计演练完成",
        E_EXPECT => "终态与场景预期不一致：注入结果偏离卡面预判",
        E_BAL => "三态计数不守恒：pass+fail+abort 必须等于 executed",
        E_SEALED => "矩阵已封账：封账后只读",
        E_PKG_TOO_BIG => "包载荷超上限：演练包不得超过 4096 字节",
        E_NO_SCEN => "矩阵为空：先登记场景卡再开演",
        _ => "未知诊断码",
    }
}

// ── 场景卡 ───────────────────────────────────────────────────
/// 三张固定场景卡的种类判别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenarioKind {
    /// 正常升级：五站全绿，终态 Activated。
    NormalUpgrade,
    /// 坏包：校验站哈希不匹配，终态 BlockedAtVerify。
    BadPackage,
    /// 协议跳变：激活站协议号跳变，终态 BlockedAtActivate。
    ProtocolJump,
}

impl ScenarioKind {
    /// 卡面预判的合法终态（演练裁决基准）。
    fn expect_station_pass(self) -> u8 {
        match self {
            ScenarioKind::NormalUpgrade => ST_REPORT,
            ScenarioKind::BadPackage => ST_VERIFY,
            ScenarioKind::ProtocolJump => ST_ACTIVATE,
        }
    }

    /// 卡面编号（1..3，落账与守恒核对用）。
    pub fn code(self) -> u8 {
        match self {
            ScenarioKind::NormalUpgrade => 1,
            ScenarioKind::BadPackage => 2,
            ScenarioKind::ProtocolJump => 3,
        }
    }

    /// 中文卡名（报告行渲染用）。
    pub fn zh(self) -> &'static str {
        match self {
            ScenarioKind::NormalUpgrade => "正常升级",
            ScenarioKind::BadPackage => "坏包注入",
            ScenarioKind::ProtocolJump => "协议跳变",
        }
    }
}

// ── 注入器 ───────────────────────────────────────────────────
/// 注入器：把场景卡的毒点编码成演练参数（确定性，构建期写死）。
#[derive(Clone, Debug)]
pub struct Injector {
    /// 注入毒包哈希偏移（0=不投毒，其余=毒哈希常数）。
    pub poison_hash: u64,
    /// 注入跳变协议号（0=不跳变）。
    pub jump_proto: u16,
}

impl Injector {
    /// 按卡面构建注入器。
    pub fn from_kind(k: ScenarioKind) -> Injector {
        match k {
            ScenarioKind::NormalUpgrade => Injector { poison_hash: 0, jump_proto: 0 },
            // 毒哈希写死：与任何合法包哈希撞车概率按构造排除。
            ScenarioKind::BadPackage => Injector { poison_hash: 0xDEAD_0001_0000_0001, jump_proto: 0 },
            // 跳变协议号写死：基准协议 7 → 毒协议 99。
            ScenarioKind::ProtocolJump => Injector { poison_hash: 0, jump_proto: 99 },
        }
    }
}

// ── 站级结论 ─────────────────────────────────────────────────
/// 单站结论：过/拦 + 细节码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StationVerdict {
    /// 站级放行。
    Pass,
    /// 站级拦截（携带诊断码）。
    Block(u16),
}

/// 演练终态：链走到哪一站收口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainEnd {
    /// 五站全绿。
    Activated,
    /// 校验站拦截。
    BlockedAtVerify,
    /// 激活站拦截。
    BlockedAtActivate,
}

impl ChainEnd {
    /// 终态编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            ChainEnd::Activated => 1,
            ChainEnd::BlockedAtVerify => 2,
            ChainEnd::BlockedAtActivate => 3,
        }
    }
}

// ── 升级包 ───────────────────────────────────────────────────
/// 模拟升级包：载荷 + 期望哈希 + 协议号。
#[derive(Clone, Debug)]
pub struct UpgradePkg {
    /// 载荷字节（非空、不超上限）。
    payload: Vec<u8>,
    /// 拉取时声明的期望哈希（毒包由注入器改写）。
    declared_hash: u64,
    /// 包携带的协议号（跳变包由注入器改写）。
    proto: u16,
}

impl UpgradePkg {
    /// 构建合法包：载荷非空、哈希即载荷哈希、协议号=基准。
    pub fn genuine(payload: &[u8]) -> Result<UpgradePkg, u16> {
        if payload.is_empty() {
            return Err(E_PKG_EMPTY);
        }
        if payload.len() > PKG_MAX_BYTES {
            return Err(E_PKG_TOO_BIG);
        }
        Ok(UpgradePkg {
            payload: Vec::from(payload),
            declared_hash: fnv1a64(payload),
            proto: PROTO_BASE,
        })
    }

    /// 注入器改写：坏包改声明哈希，协议跳变包改协议号。
    pub fn poison(&mut self, inj: &Injector) {
        if inj.poison_hash != 0 {
            self.declared_hash = inj.poison_hash;
        }
        if inj.jump_proto != 0 {
            self.proto = inj.jump_proto;
        }
    }

    /// 载荷长度。
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }
}

// ── 场景卡（账内实体）─────────────────────────────────────────
/// 一张已登记的演练场景卡。
#[derive(Clone, Debug)]
pub struct ScenarioCard {
    /// 卡号（1 起连续）。
    pub card_id: u32,
    /// 场景种类。
    pub kind: ScenarioKind,
    /// 卡面描述。
    pub title: String,
    /// 演练是否已执行。
    pub executed: bool,
    /// 终态（未执行为 0）。
    pub end_code: u8,
    /// 全链是否走通（终态与卡面预期一致）。
    pub chain_ok: bool,
}

// ── 演练矩阵（定容账本主体）───────────────────────────────────
/// 升级演练矩阵：三卡注入 → 全链五站 → 终态裁决 → 守恒封账。
pub struct DrillMatrix {
    cards: [Option<ScenarioCard>; MAX_SCEN],
    /// 已登记卡数。
    count: usize,
    /// 已执行卡数。
    executed: usize,
    /// 全链走通卡数。
    passed: usize,
    /// 全链走通但终态偏离卡面数（应为 0）。
    failed: usize,
    /// 执行中中断数（站内异常，恒应 0）。
    aborted: usize,
    /// 矩阵是否封账。
    sealed: bool,
}

impl DrillMatrix {
    /// 开矩阵：空账、未封。
    pub fn new() -> DrillMatrix {
        DrillMatrix {
            cards: core::array::from_fn(|_| None),
            count: 0,
            executed: 0,
            passed: 0,
            failed: 0,
            aborted: 0,
            sealed: false,
        }
    }

    /// 发卡：同种类卡不得重复登记。
    pub fn enroll(&mut self, kind: ScenarioKind, title: &str) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.count >= MAX_SCEN {
            return Err(E_SCEN_FULL);
        }
        if title.is_empty() {
            return Err(E_SCEN_NOTFOUND);
        }
        let mut i = 0usize;
        while i < self.count {
            if let Some(c) = &self.cards[i] {
                if c.kind == kind {
                    return Err(E_SCEN_DUP);
                }
            }
            i += 1;
        }
        let card_id = (self.count + 1) as u32;
        self.cards[self.count] = Some(ScenarioCard {
            card_id,
            kind,
            title: title.to_string(),
            executed: false,
            end_code: 0,
            chain_ok: false,
        });
        self.count += 1;
        Ok(card_id)
    }

    /// 卡定位：按卡号取下标。
    fn slot_of(&self, card_id: u32) -> Result<usize, u16> {
        if card_id == 0 || card_id as usize > self.count {
            return Err(E_SCEN_NOTFOUND);
        }
        Ok((card_id - 1) as usize)
    }

    /// 单站执行：给定站号与包状态，产出站级结论（确定性裁决）。
    fn run_station(
        station: u8,
        pkg: &UpgradePkg,
        inj: &Injector,
    ) -> StationVerdict {
        match station {
            ST_FETCH => {
                if pkg.payload_len() == 0 {
                    StationVerdict::Block(E_PKG_EMPTY)
                } else {
                    StationVerdict::Pass
                }
            }
            ST_VERIFY => {
                let actual = fnv1a64(&pkg.payload);
                let declared = if inj.poison_hash != 0 { inj.poison_hash } else { pkg.declared_hash };
                if declared != actual {
                    StationVerdict::Block(E_PKG_HASH)
                } else {
                    StationVerdict::Pass
                }
            }
            ST_STAGE => StationVerdict::Pass,
            ST_ACTIVATE => {
                if pkg.proto != PROTO_BASE || inj.jump_proto != 0 {
                    StationVerdict::Block(E_PROTO_JUMP)
                } else {
                    StationVerdict::Pass
                }
            }
            ST_REPORT => StationVerdict::Pass,
            _ => StationVerdict::Block(E_STATION_SKIP),
        }
    }

    /// 全链执行：五站逐站走，首拦即收口，返回终态。
    pub fn run_chain(pkg: &UpgradePkg, inj: &Injector) -> Result<(ChainEnd, Vec<u8>), u16> {
        let mut stations: Vec<u8> = Vec::new();
        let mut end = ChainEnd::Activated;
        let mut station = ST_FETCH;
        while station <= ST_REPORT {
            match DrillMatrix::run_station(station, pkg, inj) {
                StationVerdict::Pass => {
                    stations.push(station);
                    station += 1;
                }
                StationVerdict::Block(c) => {
                    end = match station {
                        ST_VERIFY => ChainEnd::BlockedAtVerify,
                        ST_ACTIVATE => ChainEnd::BlockedAtActivate,
                        _ => return Err(E_STATION_SKIP),
                    };
                    stations.push(station);
                    break;
                }
            }
        }
        if stations.is_empty() {
            return Err(E_CHAIN_INCOMPLETE);
        }
        Ok((end, stations))
    }

    /// 演练一张卡：构建真包 → 注入 → 全链 → 终态裁决落账。
    pub fn play(&mut self, card_id: u32, payload: &[u8]) -> Result<ChainEnd, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let s = self.slot_of(card_id)?;
        let kind = {
            let c = self.cards[s].as_ref().ok_or(E_SCEN_NOTFOUND)?;
            if c.executed {
                return Err(E_PHASE);
            }
            c.kind
        };
        let mut pkg = UpgradePkg::genuine(payload)?;
        let inj = Injector::from_kind(kind);
        pkg.poison(&inj);
        let (end, _stations) = DrillMatrix::run_chain(&pkg, &inj)?;
        // 终态与卡面预期一致 = 全链走通。
        let ok = end.code() == match kind {
            ScenarioKind::NormalUpgrade => ChainEnd::Activated.code(),
            ScenarioKind::BadPackage => ChainEnd::BlockedAtVerify.code(),
            ScenarioKind::ProtocolJump => ChainEnd::BlockedAtActivate.code(),
        };
        let card = self.cards[s].as_mut().ok_or(E_SCEN_NOTFOUND)?;
        card.executed = true;
        card.end_code = end.code();
        card.chain_ok = ok;
        self.executed += 1;
        if ok {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        Ok(end)
    }

    /// 三态守恒复核：pass+fail+abort == executed <= count。
    pub fn conserved(&self) -> bool {
        self.passed + self.failed + self.aborted == self.executed
            && self.executed <= self.count
    }

    /// 演练矩阵过：三卡全登记、全执行、全走通、守恒不破。
    pub fn matrix_assert(&self) -> Result<(), u16> {
        if self.count == 0 {
            return Err(E_NO_SCEN);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        if self.executed != self.count {
            return Err(E_CHAIN_INCOMPLETE);
        }
        if self.failed > 0 || self.aborted > 0 {
            return Err(E_EXPECT);
        }
        // 三张固定卡必须各到齐一次。
        let mut n_normal = 0u32;
        let mut n_bad = 0u32;
        let mut n_jump = 0u32;
        let mut i = 0usize;
        while i < self.count {
            if let Some(c) = &self.cards[i] {
                if !c.chain_ok {
                    return Err(E_EXPECT);
                }
                match c.kind {
                    ScenarioKind::NormalUpgrade => n_normal += 1,
                    ScenarioKind::BadPackage => n_bad += 1,
                    ScenarioKind::ProtocolJump => n_jump += 1,
                }
            }
            i += 1;
        }
        if n_normal != 1 || n_bad != 1 || n_jump != 1 {
            return Err(E_SCEN_DUP);
        }
        Ok(())
    }

    /// 封账：守恒通过后冻结矩阵。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.matrix_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(c) = &self.cards[i] {
                buf.push(c.card_id as u8);
                buf.push(c.kind.code());
                buf.push(c.end_code);
                buf.push(c.chain_ok as u8);
                buf.extend_from_slice(c.title.as_bytes());
                buf.push(0x1f);
            }
            i += 1;
        }
        buf.extend_from_slice((self.passed as u64).to_le_bytes().as_slice());
        let h = fnv1a64(&buf);
        self.sealed = true;
        Ok(h)
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 报告行渲染：逐卡"卡号|种类|终态|走通"。
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 升级演练矩阵 · 三场景全链 ===".to_string());
        let mut i = 0usize;
        while i < self.count {
            if let Some(c) = &self.cards[i] {
                let mut line = String::from("C");
                line.push_str(&itoa(c.card_id as u64));
                line.push('|');
                line.push_str(c.kind.zh());
                line.push_str("|终态");
                line.push_str(&itoa(c.end_code as u64));
                line.push_str("|走通=");
                line.push_str(if c.chain_ok { "是" } else { "否" });
                lines.push(line);
            }
            i += 1;
        }
        lines.push("走通 ".to_string() + &itoa(self.passed as u64)
            + "/" + &itoa(self.count as u64));
        lines
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：三场景全链走通 3/3 + 错误路径抽样。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut mx = DrillMatrix::new();
    let c1 = mx.enroll(ScenarioKind::NormalUpgrade, "正常升级全链")?;
    let c2 = mx.enroll(ScenarioKind::BadPackage, "坏包校验拦截")?;
    let c3 = mx.enroll(ScenarioKind::ProtocolJump, "协议跳变拦截")?;
    let pkg = [0x55u8; 128];
    // 正常升级：终态必须 Activated。
    match mx.play(c1, &pkg)? {
        ChainEnd::Activated => {}
        _ => return Err(SEG_BASE),
    }
    // 坏包：终态必须 BlockedAtVerify。
    match mx.play(c2, &pkg)? {
        ChainEnd::BlockedAtVerify => {}
        _ => return Err(SEG_BASE),
    }
    // 协议跳变：终态必须 BlockedAtActivate。
    match mx.play(c3, &pkg)? {
        ChainEnd::BlockedAtActivate => {}
        _ => return Err(SEG_BASE),
    }
    mx.matrix_assert()?;
    let h = mx.seal()?;
    if h == 0 || !mx.is_sealed() {
        return Err(SEG_BASE);
    }
    // 封账后再演练必须被拒。
    if mx.play(c1, &pkg).is_ok() {
        return Err(SEG_BASE);
    }
    // 错误路径：重复发同种卡。
    let mut mx2 = DrillMatrix::new();
    let _ = mx2.enroll(ScenarioKind::NormalUpgrade, "a")?;
    match mx2.enroll(ScenarioKind::NormalUpgrade, "b") {
        Err(c) if c == E_SCEN_DUP => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：空包在拉取站即拦（链结论 BlockedAtActivate 之外须合法）。
    match UpgradePkg::genuine(&[]) {
        Err(c) if c == E_PKG_EMPTY => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
