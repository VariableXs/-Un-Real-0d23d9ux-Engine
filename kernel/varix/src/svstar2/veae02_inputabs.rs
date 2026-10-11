//! VE-F6202 · 输入抽象层（veae02_inputabs）
//!
//! **设备无关的输入抽象**——键盘/手柄/触屏/VR 统一为抽象动作
//! [`InputAction`]（游戏逻辑只认动作不认设备——换设备不改逻辑）；
//! **设备能力声明制**（能力差异显式标注**不假装相等**——手柄没有键
//! 盘就说没有，触屏没有肩键就说没有，降级是显性路径不是猜测）；
//! **抽象动作与设备事件的映射注册表**（同动作多设备源，表是唯一事实）。
//!
//! 数据结构（锚点原文）：抽象动作表；能力声明。
//!
//! 错误路径与降级矩阵：**未声明能力→降级+告知**（缺能力的动作走降级
//! 通路并读屏告知——静默吞动作等于按键坏了没人知道）；**映射冲突→
//! 仲裁**（同设备同事件双动作——先注册优先锁定，仲裁可查）；**注册
//! 越权→拒绝**（映射表写入需 Editor 权——权限递增链）。
//!
//! 性能：派发 O(动作数)。跨批对接点：AE02/03/04 设备段（F6201 输入
//! 总架构在库——分层与三律由其承接）。无障碍：映射表读屏可达
//! （`mapping_spoken` 可读名单）。无隐私面（输入映射不含内容记录）。
//!
//! Z 域字符串码五码互异：`E_IA_NOAUTH` / `E_IA_NO_CAP` /
//! `E_IA_CONFLICT` / `E_IA_UNKNOWN_ACT` / `E_IA_UNKNOWN_DEV`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 注册越权（映射表写入需 Editor 权）。
pub const E_IA_NOAUTH: &str = "E_IA_NOAUTH";
/// 未声明能力（动作要求的能力设备未声明——降级+告知）。
pub const E_IA_NO_CAP: &str = "E_IA_NO_CAP";
/// 映射冲突（同设备同事件双动作——仲裁记录）。
pub const E_IA_CONFLICT: &str = "E_IA_CONFLICT";
/// 查无动作（未登记的抽象动作）。
pub const E_IA_UNKNOWN_ACT: &str = "E_IA_UNKNOWN_ACT";
/// 查无设备（未登记的设备 ID）。
pub const E_IA_UNKNOWN_DEV: &str = "E_IA_UNKNOWN_DEV";

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_IA_NOAUTH => "越权：映射表写入需 Editor 权限",
        E_IA_NO_CAP => "能力未声明：设备缺该动作所需能力，走降级并告知",
        E_IA_CONFLICT => "映射冲突：同设备同事件已有映射，先注册优先",
        E_IA_UNKNOWN_ACT => "查无动作：抽象动作未登记",
        E_IA_UNKNOWN_DEV => "查无设备：设备 ID 未登记能力声明",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 抽象动作与设备能力（数据面）
// ---------------------------------------------------------------------------

/// 抽象动作闭集（设备无关——游戏逻辑消费面；闭集枚举判据才闭合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputAction {
    Move,
    Jump,
    Attack,
    Interact,
    Pause,
    CameraZoom,
}

impl InputAction {
    /// 全量闭集（判据枚举对拍用）。
    pub fn all() -> [InputAction; 6] {
        [
            InputAction::Move,
            InputAction::Jump,
            InputAction::Attack,
            InputAction::Interact,
            InputAction::Pause,
            InputAction::CameraZoom,
        ]
    }

    /// 动作中文名（读屏可达——动作名单可读）。
    pub fn spoken(self) -> &'static str {
        match self {
            InputAction::Move => "移动",
            InputAction::Jump => "跳跃",
            InputAction::Attack => "攻击",
            InputAction::Interact => "交互",
            InputAction::Pause => "暂停",
            InputAction::CameraZoom => "缩放",
        }
    }

    /// 动作所需能力位（**任一命中即可派发**——动作可用多种设备方式
    /// 完成，需求位是「或」不是「与」：与语义会把多源动作错锁死在
    /// 单一设备上——移动可以摇杆/方向键/触屏滑动，缩放可以滚轮/捏合）。
    pub fn required_caps(self) -> u32 {
        match self {
            InputAction::Move => CAP_HAS_AXIS | CAP_HAS_POINTER | CAP_HAS_BUTTONS,
            InputAction::Jump => CAP_HAS_BUTTONS,
            InputAction::Attack => CAP_HAS_BUTTONS,
            InputAction::Interact => CAP_HAS_BUTTONS | CAP_HAS_POINTER,
            InputAction::Pause => CAP_HAS_BUTTONS | CAP_HAS_POINTER,
            InputAction::CameraZoom => CAP_HAS_POINTER | CAP_HAS_AXIS,
        }
    }
}

/// 设备能力位（声明制——有就是有，没有就是没有，不假装相等）。
pub const CAP_HAS_AXIS: u32 = 1 << 0; // 轴（摇杆/滚轮）
pub const CAP_HAS_BUTTONS: u32 = 1 << 1; // 按键
pub const CAP_HAS_POINTER: u32 = 1 << 2; // 指针（鼠标/触屏）
pub const CAP_HAS_TOUCH: u32 = 1 << 3; // 多点触控
pub const CAP_HAS_VR: u32 = 1 << 4; // VR 头显/手柄
pub const CAP_HAS_RUMBLE: u32 = 1 << 5; // 震动反馈

/// 设备种类闭集（映射注册表的设备面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Gamepad,
    Touch,
    Vr,
}

impl DeviceKind {
    pub fn spoken(self) -> &'static str {
        match self {
            DeviceKind::Keyboard => "键盘",
            DeviceKind::Gamepad => "手柄",
            DeviceKind::Touch => "触屏",
            DeviceKind::Vr => "VR",
        }
    }
}

/// 设备登记项（ID+种类+能力声明——声明制即登记时显式给位图）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceEntry {
    pub id: u32,
    pub kind: DeviceKind,
    pub caps: u32,
}

/// 映射条目（设备事件 → 抽象动作；event 用设备内局部码——同码不同
/// 设备不冲突，跨设备冲突在仲裁面按条目处理）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapEntry {
    pub dev: u32,
    pub event: u32,
    pub action: InputAction,
    /// 降级通路（设备缺能力时的替代告知文本——显性降级不是吞动作）。
    pub fallback: &'static str,
}

// ---------------------------------------------------------------------------
// 权限闸（注册越权→拒绝；Editor 专属写入）
// ---------------------------------------------------------------------------

/// 映射表权限三级（vek18/CIPC 同款递增链：Off < Viewer < Editor）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapAuth {
    Off,
    Viewer,
    Editor,
}

// ---------------------------------------------------------------------------
// 抽象动作表 + 映射注册表（门面）
// ---------------------------------------------------------------------------

/// 映射冲突仲裁记录（谁赢了、谁被拒——可查不静默）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arbitration {
    pub dev: u32,
    pub event: u32,
    pub winner: InputAction,
    pub loser: InputAction,
}

/// 输入抽象门面：设备登记+动作表+映射注册表+派发+仲裁+降级告知。
pub struct InputAbs {
    pub auth: MapAuth,
    devices: Vec<DeviceEntry>,
    maps: Vec<MapEntry>,
    /// 仲裁簿（冲突记录——先注册优先）。
    pub arbitrations: Vec<Arbitration>,
    /// 降级告知计数（每次降级 +1——告知面可查）。
    pub fallback_told: u32,
}

pub const DEV_CAP: usize = 16;
pub const MAP_CAP: usize = 64;

impl InputAbs {
    pub fn new(auth: MapAuth) -> Self {
        InputAbs {
            auth,
            devices: Vec::new(),
            maps: Vec::new(),
            arbitrations: Vec::new(),
            fallback_told: 0,
        }
    }

    /// 设备登记（能力声明制——登记即声明；幂等同 id 覆盖更新不重复）。
    pub fn register_device(&mut self, e: DeviceEntry) -> Result<bool, &'static str> {
        if self.devices.len() >= DEV_CAP && !self.devices.iter().any(|d| d.id == e.id) {
            return Err(E_IA_UNKNOWN_DEV); // 容量满复用未知码（资源面）
        }
        if let Some(d) = self.devices.iter_mut().find(|d| d.id == e.id) {
            *d = e; // upsert：同 id 覆盖（能力变更显式替换）
            return Ok(false);
        }
        self.devices.push(e);
        Ok(true)
    }

    fn dev_of(&self, id: u32) -> Option<&DeviceEntry> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn dev_count(&self) -> usize {
        self.devices.len()
    }

    /// 设备能力查询（声明制读面——未登记设备显性拒）。
    pub fn caps_of(&self, dev: u32) -> Result<u32, &'static str> {
        self.dev_of(dev).map(|d| d.caps).ok_or(E_IA_UNKNOWN_DEV)
    }

    /// 映射注册（Editor 专属——越权拒；冲突仲裁：同设备同事件已映射
    /// →先注册优先，后来者被拒并记仲裁簿；幂等重复 False）。
    pub fn bind(&mut self, m: MapEntry) -> Result<bool, &'static str> {
        if self.auth < MapAuth::Editor {
            return Err(E_IA_NOAUTH);
        }
        if self.dev_of(m.dev).is_none() {
            return Err(E_IA_UNKNOWN_DEV);
        }
        if self.maps.len() >= MAP_CAP {
            return Err(E_IA_CONFLICT);
        }
        if self.maps.iter().any(|x| x.dev == m.dev && x.event == m.event && x.action == m.action) {
            return Ok(false);
        }
        if let Some(win) = self.maps.iter().find(|x| x.dev == m.dev && x.event == m.event) {
            // 仲裁：先注册优先（winner=在册，loser=后来者）。
            self.arbitrations.push(Arbitration {
                dev: m.dev,
                event: m.event,
                winner: win.action,
                loser: m.action,
            });
            return Err(E_IA_CONFLICT);
        }
        self.maps.push(m);
        Ok(true)
    }

    /// 解绑（Editor 专属——映射修正通路）。
    pub fn unbind(&mut self, dev: u32, event: u32) -> Result<bool, &'static str> {
        if self.auth < MapAuth::Editor {
            return Err(E_IA_NOAUTH);
        }
        let before = self.maps.len();
        self.maps.retain(|x| !(x.dev == dev && x.event == event));
        Ok(self.maps.len() != before)
    }

    /// 映射查询（O(映射数)——表是唯一事实）。
    pub fn lookup(&self, dev: u32, event: u32) -> Option<InputAction> {
        self.maps
            .iter()
            .find(|x| x.dev == dev && x.event == event)
            .map(|x| x.action)
    }

    pub fn map_count(&self) -> usize {
        self.maps.len()
    }

    /// 动作派发（O(动作数)——设备存在性先查（未登记设备显性
    /// E_IA_UNKNOWN_DEV），再查映射（未映射 E_IA_UNKNOWN_ACT），再
    /// 能力对拍：缺能力走降级告知（fallback_told+1）返回
    /// Err(E_IA_NO_CAP)；一切正常返回 Ok(动作)）。
    pub fn dispatch(&mut self, dev: u32, event: u32) -> Result<InputAction, &'static str> {
        let caps = self.caps_of(dev)?;
        let action = match self.lookup(dev, event) {
            Some(a) => a,
            None => return Err(E_IA_UNKNOWN_ACT),
        };
        if caps & action.required_caps() == 0 {
            // 降级+告知：告知面计数（静默吞动作等于按键坏了没人知道）。
            self.fallback_told += 1;
            return Err(E_IA_NO_CAP);
        }
        Ok(action)
    }

    /// 降级告知文本（派发失败时调用方取——显性告知面）。
    pub fn fallback_of(&self, dev: u32, event: u32) -> Option<&'static str> {
        self.maps
            .iter()
            .find(|x| x.dev == dev && x.event == event)
            .map(|x| x.fallback)
    }

    /// 映射表读屏可达（无障碍：族→设备→动作可读名单——O(映射数)）。
    pub fn mapping_spoken(&self) -> String {
        let mut out = String::from("输入映射：");
        for d in self.devices.iter() {
            let n = self.maps.iter().filter(|m| m.dev == d.id).count();
            out.push_str(&format!("[{}]{}条 ", d.kind.spoken(), n));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 判据（锚点逐条：抽象动作/能力声明/映射注册/差异诚实/判据）
// ---------------------------------------------------------------------------

/// F6202 判据集（9 条，全部数值手算复核、零 panic 面）。
pub fn run_veae02_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6202");

    // --- 抽象动作（判据一）---
    // 动-01：动作闭集 6 枚举互异+中文名互异非空（读屏面）——逻辑只认
    // 动作不认设备的消费面闭式。
    let acts = InputAction::all();
    let mut distinct = true;
    for i in 0..acts.len() {
        for j in (i + 1)..acts.len() {
            if acts[i] == acts[j] || acts[i].spoken() == acts[j].spoken() {
                distinct = false;
            }
        }
    }
    let act_ok = distinct && acts.iter().all(|a| !a.spoken().is_empty());
    if act_ok {
        s.ok("F6202-动-01-抽象动作闭集互异");
    } else {
        s.fail("F6202-动-01-抽象动作闭集互异", "抽象动作失效");
    }

    // --- 能力声明（判据二）---
    // 能-01：声明制登记——手柄（轴+按键+震动）键盘（按键+指针）触屏
    // （指针+触控）caps 位图对拍；upsert 同 id 覆盖不重复建账
    // （dev_count 恒 2）；未登记设备 caps 查询显性拒。
    let mut ia2 = InputAbs::new(MapAuth::Editor);
    let _ = ia2.register_device(DeviceEntry { id: 1, kind: DeviceKind::Gamepad, caps: CAP_HAS_AXIS | CAP_HAS_BUTTONS | CAP_HAS_RUMBLE });
    let _ = ia2.register_device(DeviceEntry { id: 2, kind: DeviceKind::Keyboard, caps: CAP_HAS_BUTTONS | CAP_HAS_POINTER });
    let caps1 = ia2.caps_of(1).expect("probe-2a");
    let caps2 = ia2.caps_of(2).expect("probe-2b");
    let _ = ia2.register_device(DeviceEntry { id: 2, kind: DeviceKind::Keyboard, caps: CAP_HAS_BUTTONS }); // upsert 覆盖
    let caps2b = ia2.caps_of(2).expect("probe-2c");
    let nocap = ia2.caps_of(9);
    let cap_ok = caps1 == CAP_HAS_AXIS | CAP_HAS_BUTTONS | CAP_HAS_RUMBLE
        && caps2 == CAP_HAS_BUTTONS | CAP_HAS_POINTER
        && caps2b == CAP_HAS_BUTTONS // 覆盖生效
        && ia2.dev_count() == 2
        && matches!(nocap, Err(E_IA_UNKNOWN_DEV));
    if cap_ok {
        s.ok("F6202-能-01-能力声明制upsert");
    } else {
        s.fail("F6202-能-01-能力声明制upsert", "能力声明失效");
    }

    // --- 映射注册（判据三）---
    // 映-01：Editor 注册三映射（键盘 W→Move 空格→Jump 鼠标滚轮→Zoom）
    // 查表对拍；幂等重复 False；O(动作数) 表计数恰 3。
    let mut ia3 = InputAbs::new(MapAuth::Editor);
    let _ = ia3.register_device(DeviceEntry { id: 1, kind: DeviceKind::Keyboard, caps: CAP_HAS_BUTTONS | CAP_HAS_POINTER | CAP_HAS_AXIS });
    let b1 = ia3.bind(MapEntry { dev: 1, event: 0x11, action: InputAction::Move, fallback: "方向键同样可移动" });
    let b2 = ia3.bind(MapEntry { dev: 1, event: 0x39, action: InputAction::Jump, fallback: "点按跳跃键" });
    let b3 = ia3.bind(MapEntry { dev: 1, event: 0x0E, action: InputAction::CameraZoom, fallback: "双指缩放" });
    let dup = ia3.bind(MapEntry { dev: 1, event: 0x11, action: InputAction::Move, fallback: "x" });
    let look = ia3.lookup(1, 0x11);
    let bind_ok = matches!(b1, Ok(true))
        && matches!(b2, Ok(true))
        && matches!(b3, Ok(true))
        && matches!(dup, Ok(false))
        && look == Some(InputAction::Move)
        && ia3.map_count() == 3;
    if bind_ok {
        s.ok("F6202-映-01-映射注册查表对拍");
    } else {
        s.fail("F6202-映-01-映射注册查表对拍", "映射注册失效");
    }

    // --- 映射冲突仲裁（判据四）---
    // 冲-01：同设备同事件双动作——先注册优先（winner=Move loser=
    // Attack）仲裁簿内容精确；后来者 Err(CONFLICT) 不入表；解绑后
    // 再绑成功（修正通路）；解绑不存在返回 False。
    let mut ia4 = InputAbs::new(MapAuth::Editor);
    let _ = ia4.register_device(DeviceEntry { id: 1, kind: DeviceKind::Gamepad, caps: CAP_HAS_BUTTONS | CAP_HAS_AXIS });
    let _ = ia4.bind(MapEntry { dev: 1, event: 0, action: InputAction::Move, fallback: "f" });
    let conflict = ia4.bind(MapEntry { dev: 1, event: 0, action: InputAction::Attack, fallback: "f" });
    let arb = &ia4.arbitrations[0];
    let arb_ok = matches!(conflict, Err(E_IA_CONFLICT))
        && arb.dev == 1
        && arb.event == 0
        && arb.winner == InputAction::Move
        && arb.loser == InputAction::Attack
        && ia4.lookup(1, 0) == Some(InputAction::Move);
    let unbound = ia4.unbind(1, 0).expect("probe-4a");
    let _ = ia4.bind(MapEntry { dev: 1, event: 0, action: InputAction::Attack, fallback: "f" });
    let reborn = ia4.lookup(1, 0);
    let nobind = ia4.unbind(1, 99).expect("probe-4b");
    let resolve_ok = arb_ok && unbound && reborn == Some(InputAction::Attack) && !nobind;
    if resolve_ok {
        s.ok("F6202-冲-01-冲突仲裁先注优先");
    } else {
        s.fail("F6202-冲-01-冲突仲裁先注优先", "仲裁失效");
    }

    // --- 差异诚实（判据五）---
    // 诚-01：未声明能力→降级+告知——键盘无震动：Attack 需按键（有）
    // 正常派发；构造缺按键设备（纯轴手柄）绑 Attack → 派发
    // Err(E_IA_NO_CAP) + fallback_told=1 + 降级告知文本可取
    // （不假装相等——缺就说缺，降级有告知不是吞动作）。
    let mut ia5 = InputAbs::new(MapAuth::Editor);
    let _ = ia5.register_device(DeviceEntry { id: 1, kind: DeviceKind::Gamepad, caps: CAP_HAS_AXIS }); // 纯轴无键
    let _ = ia5.bind(MapEntry { dev: 1, event: 0, action: InputAction::Attack, fallback: "攻击键不可用：设备无按键，已转语音提示" });
    let denied = ia5.dispatch(1, 0);
    let told = ia5.fallback_of(1, 0);
    let honest_ok = matches!(denied, Err(E_IA_NO_CAP))
        && ia5.fallback_told == 1
        && told.is_some();
    if honest_ok {
        s.ok("F6202-诚-01-未声明能力降级告知");
    } else {
        s.fail("F6202-诚-01-未声明能力降级告知", "差异诚实失效");
    }

    // --- 正常派发（判据六）---
    // 派-01：能力齐备派发 Ok(动作)——手柄轴+键：左摇杆→Move（需轴）
    // Ok、A 键→Jump（需键）Ok；未映射事件拒 E_IA_UNKNOWN_ACT；
    // 未登记设备拒 E_IA_UNKNOWN_DEV。
    let mut ia6 = InputAbs::new(MapAuth::Editor);
    let _ = ia6.register_device(DeviceEntry { id: 1, kind: DeviceKind::Gamepad, caps: CAP_HAS_AXIS | CAP_HAS_BUTTONS });
    let _ = ia6.bind(MapEntry { dev: 1, event: 0, action: InputAction::Move, fallback: "f" });
    let _ = ia6.bind(MapEntry { dev: 1, event: 1, action: InputAction::Jump, fallback: "f" });
    let d_move = ia6.dispatch(1, 0);
    let d_jump = ia6.dispatch(1, 1);
    let d_nomap = ia6.dispatch(1, 99);
    let d_nodev = ia6.dispatch(9, 0);
    let disp_ok = d_move == Ok(InputAction::Move)
        && d_jump == Ok(InputAction::Jump)
        && matches!(d_nomap, Err(E_IA_UNKNOWN_ACT))
        && matches!(d_nodev, Err(E_IA_UNKNOWN_DEV));
    if disp_ok {
        s.ok("F6202-派-01-能力齐备派发恰值");
    } else {
        s.fail("F6202-派-01-能力齐备派发恰值", "派发失效");
    }

    // --- 注册越权（判据七）---
    // 权-01：Off/Viewer 写映射全拒 E_IA_NOAUTH；Editor 放行——权限
    // 递增链（Viewer 可看表不可写——读屏可达不受影响）。
    let mut ia7 = InputAbs::new(MapAuth::Off);
    let _ = ia7.register_device(DeviceEntry { id: 1, kind: DeviceKind::Keyboard, caps: CAP_HAS_BUTTONS });
    let off_b = ia7.bind(MapEntry { dev: 1, event: 0, action: InputAction::Pause, fallback: "f" });
    let off_u = ia7.unbind(1, 0);
    ia7.auth = MapAuth::Viewer;
    let view_b = ia7.bind(MapEntry { dev: 1, event: 0, action: InputAction::Pause, fallback: "f" });
    let view_spoken = ia7.mapping_spoken();
    ia7.auth = MapAuth::Editor;
    let ed_b = ia7.bind(MapEntry { dev: 1, event: 0, action: InputAction::Pause, fallback: "f" });
    let auth_ok = matches!(off_b, Err(E_IA_NOAUTH))
        && matches!(off_u, Err(E_IA_NOAUTH))
        && matches!(view_b, Err(E_IA_NOAUTH))
        && view_spoken.contains("[键盘]0条")
        && matches!(ed_b, Ok(true));
    if auth_ok {
        s.ok("F6202-权-01-注册越权拒绝");
    } else {
        s.fail("F6202-权-01-注册越权拒绝", "权限闸失效");
    }

    // --- 判据元（判据八）---
    // 元-01：五码互异 + reason 互异非空 + 缺省兜底 + 零 panic 空态
    // （空表派发显性拒不是静默；空表读屏名单合规）。
    let codes = [E_IA_NOAUTH, E_IA_NO_CAP, E_IA_CONFLICT, E_IA_UNKNOWN_ACT, E_IA_UNKNOWN_DEV];
    let mut codes_distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] || reason_of(codes[i]) == reason_of(codes[j]) {
                codes_distinct = false;
            }
        }
    }
    let meta_codes = codes_distinct
        && reason_of("E_IA_UNKNOWN") == "未知诊断码"
        && codes.iter().all(|c| !reason_of(c).is_empty());
    let ia8 = InputAbs::new(MapAuth::Viewer);
    let empty_d = { let mut x = InputAbs::new(MapAuth::Viewer); x.dispatch(1, 0) };
    let empty_ok = matches!(empty_d, Err(E_IA_UNKNOWN_DEV))
        && ia8.mapping_spoken().contains("输入映射：")
        && ia8.map_count() == 0;
    if meta_codes && empty_ok {
        s.ok("F6202-元-01-五码互异空态显性");
    } else {
        s.fail("F6202-元-01-五码互异空态显性", "判据元失效");
    }

    // --- 编排（判据九）---
    // 编-01：全链路 O(动作数)——三设备（键盘/手柄/触屏）七映射：
    // 键盘 3+手柄 2+触屏 2；跨设备同动作多源（Move 键盘 WASD+手柄摇杆）
    // 表计数 7；读屏名单对账（[键盘]3条 [手柄]2条 [触屏]2条）；
    // 各设备派发抽查 Ok/降级混合对账（触屏 Jump 需按键缺→降级）。
    let mut ia9 = InputAbs::new(MapAuth::Editor);
    let _ = ia9.register_device(DeviceEntry { id: 1, kind: DeviceKind::Keyboard, caps: CAP_HAS_BUTTONS | CAP_HAS_POINTER | CAP_HAS_AXIS });
    let _ = ia9.register_device(DeviceEntry { id: 2, kind: DeviceKind::Gamepad, caps: CAP_HAS_AXIS | CAP_HAS_BUTTONS });
    let _ = ia9.register_device(DeviceEntry { id: 3, kind: DeviceKind::Touch, caps: CAP_HAS_POINTER | CAP_HAS_TOUCH });
    for m in [
        MapEntry { dev: 1, event: 0x11, action: InputAction::Move, fallback: "f" },
        MapEntry { dev: 1, event: 0x39, action: InputAction::Jump, fallback: "f" },
        MapEntry { dev: 1, event: 0x0E, action: InputAction::CameraZoom, fallback: "f" },
        MapEntry { dev: 2, event: 0, action: InputAction::Move, fallback: "f" },
        MapEntry { dev: 2, event: 1, action: InputAction::Attack, fallback: "f" },
        MapEntry { dev: 3, event: 5, action: InputAction::Move, fallback: "f" },
        MapEntry { dev: 3, event: 6, action: InputAction::Jump, fallback: "跳跃不可用：触屏无实体键，已转双击手势提示" },
    ] {
        let _ = ia9.bind(m);
    }
    let spoken9 = ia9.mapping_spoken();
    let d_kb = ia9.dispatch(1, 0x11);
    let d_pad = ia9.dispatch(2, 0);
    let d_touch_ok = ia9.dispatch(3, 5);
    let d_touch_no = ia9.dispatch(3, 6);
    let chain_ok = ia9.map_count() == 7
        && ia9.arbitrations.is_empty()
        && d_kb == Ok(InputAction::Move)
        && d_pad == Ok(InputAction::Move) // 跨设备同动作多源
        && d_touch_ok == Ok(InputAction::Move)
        && matches!(d_touch_no, Err(E_IA_NO_CAP))
        && ia9.fallback_told == 1
        && spoken9.contains("[键盘]3条")
        && spoken9.contains("[手柄]2条")
        && spoken9.contains("[触屏]2条");
    if chain_ok {
        s.ok("F6202-编-01-全链路O动作数对账");
    } else {
        s.fail("F6202-编-01-全链路O动作数对账", "编排失效");
    }

    s
}
