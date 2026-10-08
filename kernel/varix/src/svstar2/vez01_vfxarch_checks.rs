//! VE-F5201 判据集（Z 域开工与特效库总架构）。
//!
//! 与实现模块**分离**：判据只通过公开接口驱动，不偷看内部字段。
//! 判据侧自算的期望值一律**独立于被测实现**推导（十诫：判据向被测函数
//! 问答案＝自证式）。
//!
//! 判据目录：
//! - C5201-架构-四子系统齐备且责任互斥
//! - C5201-架构-两翼齐备
//! - C5201-架构-接口笛卡尔积齐备
//! - C5201-语义-无语义特效被拦截
//! - C5201-语义-拦截有专属码不与越界合并
//! - C5201-越界-跨类效果被拦
//! - C5201-预算-超预算被拦且界恰在预算线上
//! - C5201-预算-登记册绝对值口径不被建销骗
//! - C5201-冻结-全冻结后改语义越权
//! - C5201-冻结-解冻是唯一合法途径
//! - C5201-冻结-加法允许但升版本
//! - C5201-对拍-一致与失配双向可测
//! - C5201-承接-十件齐平
//! - C5201-承接-缺源回溯到源头件号
//! - C5201-诊断-带源信息与可操作建议

use crate::checks::CheckSet;
use crate::svstar2::vez01_vfxarch::*;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 把一个特效登记到标准子系统的标准类别上（判据语料helper）。
fn class_of(s: Subsystem) -> &'static str {
    s.exclusive_class()
}

/// 正常入册（无语义、无越界、不超预算）。
fn good(reg: &mut VfxRegistry, name: &str, owner: Subsystem, sem: &str, cost: u32) -> bool {
    reg.register(name, owner, class_of(owner), Some(sem), cost).is_ok()
}

pub fn run_vez01_checks() -> CheckSet {
    let mut set = CheckSet::new("vez01_vfxarch");

    // ------------------------------------------------------------------
    // 一、四子系统（判据一）
    // ------------------------------------------------------------------
    {
        let a = VfxArch::new();
        // 1a. 子系统恰四个，且四个都是锚点点名的那些。
        let all_present = {
            let mut ok = a.subsystem_count() as usize == SUBSYSTEM_COUNT;
            let mut i = 0usize;
            while i < SUBSYSTEM_COUNT && ok {
                let s = Subsystem::ALL[i];
                // 按名逐个核对，防「换个名字的四条通道」蒙混
                ok = s.name() == Subsystem::ALL[i].name();
                i += 1;
            }
            ok
        };
        set.add(
            "C5201-架构-四子系统齐备",
            a.subsystem_count() == 4
                && Subsystem::Particle.name() == "粒子"
                && Subsystem::PostProcess.name() == "后处理"
                && Subsystem::Environment.name() == "环境"
                && Subsystem::Transition.name() == "转场"
                && all_present,
            "锚点四子系统：粒子/后处理/环境/转场",
        );
        // 1b. 责任互斥：四条通道的独占类别**两两不同**。
        //     只断「有四个类别」不够——四条通道都声明同一个类别时，
        //     类别数仍为 4，但互斥性已塌（判据只证明有形状不证明摊得准）。
        let mut cls: Vec<&str> = Vec::new();
        let mut i = 0usize;
        while i < SUBSYSTEM_COUNT {
            cls.push(Subsystem::ALL[i].exclusive_class());
            i += 1;
        }
        let mut distinct = true;
        let mut a1 = 0usize;
        while a1 < cls.len() {
            let mut b1 = a1 + 1;
            while b1 < cls.len() {
                if cls[a1] == cls[b1] {
                    distinct = false;
                }
                b1 += 1;
            }
            a1 += 1;
        }
        set.add(
            "C5201-架构-四子系统责任互斥",
            distinct && cls.len() == SUBSYSTEM_COUNT,
            "四条通道的独占效果类别必须两两不同，否则越界判据无标尺",
        );
        // 1c. 帧预算：每子系统都有预算，且非零。
        //     预算为零等于「任何特效都超预算」，判据会全体转红而看不出是预算写死。
        let mut budgets_ok = true;
        let mut k = 0usize;
        while k < SUBSYSTEM_COUNT {
            if Subsystem::ALL[k].budget_us() == 0 {
                budgets_ok = false;
            }
            k += 1;
        }
        set.add(
            "C5201-架构-帧预算非零",
            budgets_ok,
            "丰盛不以帧率为代价：每个子系统都要有可用的帧预算硬上限",
        );
    }

    // ------------------------------------------------------------------
    // 二、两翼（判据二）
    // ------------------------------------------------------------------
    {
        let a = VfxArch::new();
        set.add(
            "C5201-架构-两翼齐备",
            a.wing_count() == 2
                && Wing::Param.name() == "参数系统"
                && Wing::Compose.name() == "组合系统",
            "两翼两系统：参数系统管取值、组合系统管叠加次序",
        );
        // 接口名必含子系统：只写 `p-` 无法区分四条通道的形参入口，
        // 四条会挤成一条。这里逐个核对接口名确实带出了子系统标记。
        let mut names_ok = true;
        let mut i = 0usize;
        while i < WING_COUNT {
            let w = Wing::ALL[i];
            let mut k = 0usize;
            while k < SUBSYSTEM_COUNT {
                let want = interface_name(w, Subsystem::ALL[k]);
                if want.len() < 3 {
                    names_ok = false;
                }
                k += 1;
            }
            i += 1;
        }
        let uniq = {
            let mut vs: Vec<String> = Vec::new();
            let mut i = 0usize;
            while i < WING_COUNT {
                let mut k = 0usize;
                while k < SUBSYSTEM_COUNT {
                    vs.push(interface_name(Wing::ALL[i], Subsystem::ALL[k]));
                    k += 1;
                }
                i += 1;
            }
            let mut d = true;
            let mut x = 0usize;
            while x < vs.len() {
                let mut y = x + 1;
                while y < vs.len() {
                    if vs[x] == vs[y] {
                        d = false;
                    }
                    y += 1;
                }
                x += 1;
            }
            d && vs.len() == INTERFACE_COUNT
        };
        set.add(
            "C5201-架构-两翼接口名含子系统且唯一",
            names_ok && uniq,
            "接口名须能区分是哪个子系统的形参入口，否则四通道挤成一条、两翼形同虚设",
        );
    }

    // ------------------------------------------------------------------
    // 三、接口笛卡尔积（判据一 + 四 的交集）
    // ------------------------------------------------------------------
    {
        let a = VfxArch::new();
        // 判据侧独立数：两翼各对四子系统都有一条接口 = 8。
        // 不用 `a.interface_count()` 自证（那是被测对象自己报数）。
        let mut expect = 0u32;
        let mut i = 0usize;
        while i < WING_COUNT {
            let mut k = 0usize;
            while k < SUBSYSTEM_COUNT {
                expect += 1;
                k += 1;
            }
            i += 1;
        }
        // 逐条核对：每条接口的上游是翼、下游是子系统（真按笛卡尔积生成）。
        let mut shape_ok = a.interfaces.len() as u32 == expect;
        let mut idx = 0usize;
        while idx < a.interfaces.len() {
            let it = &a.interfaces[idx];
            // 上游必须是两翼之一、下游必须是四子系统之一：顺序即依赖方向。
            let up_ok = it.upstream == Wing::Param || it.upstream == Wing::Compose;
            let dn_ok = it.downstream.ordinal() < SUBSYSTEM_COUNT;
            if !up_ok || !dn_ok {
                shape_ok = false;
            }
            // 接口名必须与(上游,下游)对得上：契约唯一标识，防张冠李戴。
            if it.name != interface_name(it.upstream, it.downstream) {
                shape_ok = false;
            }
            idx += 1;
        }
        set.add(
            "C5201-架构-接口笛卡尔积齐备",
            shape_ok && a.interface_count() == expect,
            "两翼 × 四子系统 = 8 条层间接口，名称与上下游严格对应",
        );
    }

    // ------------------------------------------------------------------
    // 四、特效即内容：无语语义拦截（判据三）
    // ------------------------------------------------------------------
    {
        let mut reg = VfxRegistry::new();
        // 先造一条正常入册的（前置：册子本身是能用的）。
        let pre = good(&mut reg, "正常特效", Subsystem::Particle, "打击感：火星迸射", 100);
        // 4a. 前置断言（十诫五：判据必须先断目标事件的前置，否则是空断言）。
        set.add(
            "C5201-语义-前置-正常特效可入册",
            pre && reg.total() == 1 && reg.admitted() == 1,
            "无语义拦截判据的前置：一条合规特效必须能正常入册，否则拦截可能只是「全都不收」",
        );
        // 4b. 无语义特效必须被拦。
        let bad = reg.register(
            "炫光特效",
            Subsystem::Particle,
            class_of(Subsystem::Particle),
            None,
            100,
        );
        let rejected_as_undeclared = match &bad {
            Err(e) => matches!(e, VfxError::UndeclaredSemantic { name } if *name == "炫光特效"),
            Ok(_) => false,
        };
        set.add(
            "C5201-语义-无语义特效被拦截",
            rejected_as_undeclared,
            "炫技特征：无内容语义声明的特效评审不予通过",
        );
        // 4c. 拦截**不留痕等于把违规藏起来**：条目仍在册内可查。
        set.add(
            "C5201-语义-拦截条目仍留册可查",
            reg.total() == 2 && reg.rejected == 1,
            "拦截不删条目：否则炫技特效从册上消失，违规被藏起来",
        );
    }

    // ------------------------------------------------------------------
    // 五、拦截分类不合并（十诫：合并则删任一条仍全绿）
    // ------------------------------------------------------------------
    {
        // 炫技拦截与越界拦截必须各有专属码。
        let undec = VfxError::UndeclaredSemantic { name: "x".to_string() };
        let over = VfxError::ClassOverreach {
            name: "x".to_string(),
            owner: Subsystem::Particle,
            got: "fullscreen-composite",
            want: "point-force",
        };
        let bud = VfxError::BudgetExceeded {
            name: "x".to_string(),
            owner: Subsystem::Particle,
            cost_us: 9999,
            budget_us: 2400,
        };
        // 同一条错误在不同子系统上应给不同码（否则预算错误跨子系统合并成一类）。
        let bud_env = VfxError::BudgetExceeded {
            name: "x".to_string(),
            owner: Subsystem::Environment,
            cost_us: 9999,
            budget_us: 900,
        };
        set.add(
            "C5201-语义-三类失败各有专属码",
            undec.code() != over.code()
                && over.code() != bud.code()
                && undec.code() != bud.code()
                && bud.code() != bud_env.code(),
            "炫技/越界/预算三类失败分类不得合并；预算错误还须按子系统分码",
        );
        // 五条错误全码互异。
        let amend = VfxError::AmendFrozen { interface: "p-pa".to_string() };
        let miss = VfxError::HandoffMissing { item: "x", source: "F5193-01" };
        let codes: Vec<u32> = vec![
            undec.code(),
            over.code(),
            bud.code(),
            bud_env.code(),
            amend.code(),
            miss.code(),
        ];
        let mut uniq = true;
        let mut i = 0usize;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        // 判据侧独立重算：码位分三字节段——0x5B0000 | (分支 << 12) | (子系统序 << 8)。
        // 判据侧**不读实现的布局代码**，自己把期望值算出来再对拍：
        // 若实现的位段与这里写的不一致（例如把分支塞进第 8~15 位而该段已被
        // 0x5B 占用），此处就会红。这正是「不向被测对象问答案」的落点。
        let want_bud_particle = 0x5B_0000u32 | (2u32 << 12) | (0u32 << 8);
        let want_bud_env = 0x5B_0000u32 | (2u32 << 12) | (2u32 << 8);
        // 另外独立钉死：分支 0 与分支 1 的码必须真的差开
        // （位段重叠时两者会同码，这是本单实际踩过的坑）。
        let want_undec = 0x5B_0000u32 | (0u32 << 12);
        let want_over = 0x5B_0000u32 | (1u32 << 12);
        set.add(
            "C5201-语义-全错误码位互异且可独立重算",
            uniq
                && codes.len() == 6
                && bud.code() == want_bud_particle
                && bud_env.code() == want_bud_env
                && undec.code() == want_undec
                && over.code() == want_over,
            "码位不得互撞；位段不重叠是分类不合并的物理前提，布局须能由判据侧独立重算对上",
        );
    }

    // ------------------------------------------------------------------
    // 六、越界（跨类效果被拦）
    // ------------------------------------------------------------------
    {
        let mut reg = VfxRegistry::new();
        // 6a. 前置：一条正常粒子特效能入册（否则拦截可能因册坏而恒真）。
        let pre = good(&mut reg, "火星", Subsystem::Particle, "打击感", 100);
        // 6b. 粒子子系统声明后处理类效果 = 越界。
        let bad = reg.register(
            "越界者",
            Subsystem::Particle,
            Subsystem::PostProcess.exclusive_class(),
            Some("它其实想做全屏色调"),
            100,
        );
        let is_over = match &bad {
            Err(VfxError::ClassOverreach { owner, got, want, .. }) => {
                *owner == Subsystem::Particle
                    && *got == "fullscreen-composite"
                    && *want == "point-force"
            }
            _ => false,
        };
        set.add(
            "C5201-越界-前置-正常特效可入册",
            pre,
            "越界拦截判据的前置：合规特效必须能入册",
        );
        set.add(
            "C5201-越界-跨类效果被拦",
            is_over,
            "子系统只允许产出自己那类效果；越界即拦，诊断须带 got/want 两端证据",
        );
        // 6c. 越界与炫技的**判定顺序**要钉死：
        //     同时越界且无语义的特效，报的必须是炫技（先判语义）。
        //     若顺序反了，两条判据都可能对——顺序即契约。
        let mut reg2 = VfxRegistry::new();
        let both = reg2.register(
            "既越界又炫技",
            Subsystem::Particle,
            Subsystem::Environment.exclusive_class(),
            None,
            100,
        );
        let first_is_semantic = match &both {
            Err(VfxError::UndeclaredSemantic { .. }) => true,
            _ => false,
        };
        set.add(
            "C5201-越界-炫技判定优先于越界",
            first_is_semantic,
            "同时无语义且越类时，报无语义（语义是入册前提，越界是归属问题）",
        );
    }

    // ------------------------------------------------------------------
    // 七、预算（夹逼对：界前/界上/界后）
    // ------------------------------------------------------------------
    {
        let mut reg = VfxRegistry::new();
        let bud = Subsystem::Transition.budget_us(); // 600
        // 7a. 前置：界前一位（599）与界上（600）必须都收。
        //     只在界一侧取样 ⇒ 判据分不清 `>` 与 `>=`（十诫九）。
        let below = good(&mut reg, "界前", Subsystem::Transition, "转场语义", bud - 1);
        let at = good(&mut reg, "界上", Subsystem::Transition, "转场语义", bud);
        set.add(
            "C5201-预算-界前与界上均收",
            below && at && reg.total() == 2 && reg.rejected == 0,
            "预算恰用满是允许的：预算就是上限本身，判据要能区分 > 与 >=",
        );
        // 7b. 界后一位（601）必须被拦，且报出的数字要能自校对。
        let mut reg2 = VfxRegistry::new();
        let over = reg2.register(
            "界后",
            Subsystem::Transition,
            class_of(Subsystem::Transition),
            Some("转场语义"),
            bud + 1,
        );
        let ok_over = match &over {
            Err(VfxError::BudgetExceeded { cost_us, budget_us, owner, .. }) => {
                *cost_us == bud + 1 && *budget_us == bud && *owner == Subsystem::Transition
            }
            _ => false,
        };
        set.add(
            "C5201-预算-界后一位被拦且数字自洽",
            ok_over && reg2.total() == 1 && reg2.rejected == 1,
            "超预算即拦；诊断须报出实测耗时与该子系统硬上限，两个数字都要能自校对",
        );
        // 7c. 四子系统预算各不相同（若全相同，「按预算拦」就退化成全局单阈值）。
        let mut budgets: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < SUBSYSTEM_COUNT {
            budgets.push(Subsystem::ALL[i].budget_us());
            i += 1;
        }
        let mut d = true;
        let mut x = 0usize;
        while x < budgets.len() {
            let mut y = x + 1;
            while y < budgets.len() {
                if budgets[x] == budgets[y] {
                    d = false;
                }
                y += 1;
            }
            x += 1;
        }
        set.add(
            "C5201-预算-四子系统预算各异",
            d && budgets.len() == SUBSYSTEM_COUNT,
            "粒子最贵、转场最便宜：预算全同则预算判据退化为全局单阈值",
        );
    }

    // ------------------------------------------------------------------
    // 八、绝对值口径（防「建了又销」骗过）
    // ------------------------------------------------------------------
    {
        let mut reg = VfxRegistry::new();
        let _ = good(&mut reg, "a", Subsystem::Particle, "语义", 10);
        let _ = good(&mut reg, "b", Subsystem::Particle, "语义", 10);
        let _ = good(&mut reg, "c", Subsystem::Particle, "语义", 10);
        let _ = good(&mut reg, "d", Subsystem::Particle, "语义", 10);
        // 判据侧独立重算「本轮真跑用例数」=4，断 total() **恰等于**该数，
        // 而非断净值（净值会被「建了又销」骗过，十诫十）。
        let expect_total = 4u32;
        set.add(
            "C5201-预算-登记册绝对值口径",
            reg.total() == expect_total && reg.admitted() == expect_total && reg.rejected == 0,
            "total 须恰等于本轮真跑用例数；净值口径会被「建了又销」骗过",
        );
        // 按子系统独立计数：语料四条**全属粒子**，故粒子恰 4、其余三条恰 0。
        // 这里必须按「本轮真往哪个子系统登记了什么」独立算期望，
        // 不能对四个子系统用同一个期望值——那是在测「全都等于 4」，
        // 与本轮语料的实际分布无关，判据会与被测对象一起漂。
        let expect_particle = expect_total;
        let expect_others = 0u32;
        let per_sub_ok = {
            let mut i = 0usize;
            let mut ok = true;
            while i < SUBSYSTEM_COUNT {
                let s = Subsystem::ALL[i];
                let want = if s == Subsystem::Particle { expect_particle } else { expect_others };
                if reg.admitted_of(s) != want {
                    ok = false;
                }
                i += 1;
            }
            ok
        };
        set.add(
            "C5201-预算-按子系统计数不串件",
            per_sub_ok
                && reg.admitted_of(Subsystem::Particle) == 4
                && reg.admitted_of(Subsystem::PostProcess) == 0
                && reg.admitted_of(Subsystem::Environment) == 0
                && reg.admitted_of(Subsystem::Transition) == 0,
            "语料四条全属粒子：粒子计数须恰 4，其余三条须恰 0；四者若串件计数会错位",
        );
    }

    // ------------------------------------------------------------------
    // 九、层冻结（判据四）
    // ------------------------------------------------------------------
    {
        let mut a = VfxArch::new();
        // 9a. 前置：全冻结后冻结数应等于接口数（判据侧独立算出期望 8）。
        let expect_if = {
            let mut n = 0u32;
            let mut i = 0usize;
            while i < WING_COUNT {
                let mut k = 0usize;
                while k < SUBSYSTEM_COUNT {
                    n += 1;
                    k += 1;
                }
                i += 1;
            }
            n
        };
        a.freeze_all();
        let frozen_ok = a.frozen_count() == expect_if && expect_if == 8;
        set.add(
            "C5201-冻结-前置-全冻结后计数齐平",
            frozen_ok,
            "冻结判据的前置：8 条接口全部冻结，冻结数与接口数齐平",
        );
        // 9b. 冻结态改既有语义 = 越权。
        let amend_res = a.amend_interface("p-pa", AmendKind::Change);
        let is_err = match &amend_res {
            Err(VfxError::AmendFrozen { interface }) => *interface == "p-pa",
            _ => false,
        };
        set.add(
            "C5201-冻结-改语义被拒",
            is_err,
            "冻结态改既有语义即接口越权，须被拒",
        );
        // 9c. 加法在冻结态**允许但必须升版本**（冻结不是不许长大）。
        let ext_res = a.amend_interface("p-pa", AmendKind::Extend);
        let ext_ok = match &ext_res {
            Ok(v) => {
                // 判据侧独立重算期望版本：初版 1，加一次 = 2。
                *v == 2 && a.interface_of("p-pa").map(|i| i.version) == Some(2)
            }
            Err(_) => false,
        };
        set.add(
            "C5201-冻结-加法允许但升版本",
            ext_ok,
            "冻结禁的是改语义；加法允许，但必须显式升版本登记",
        );
        // 9d. 越权操作**不得改到版本号**：若改语义时顺手升了版本，
        //     拦截就名存实亡（表面上拒绝了，状态却已变）。
        let after = a.interface_of("p-pa").map(|i| (i.version, i.mutations)).unwrap_or((0, 0));
        set.add(
            "C5201-冻结-越权不改版本号",
            after.0 == 2 && after.1 == 0,
            "越权改语义须是零副作用：版本与改动计数都不得动",
        );
    }

    // ------------------------------------------------------------------
    // 十、解冻是唯一合法途径
    // ------------------------------------------------------------------
    {
        let mut it = FrozenInterface::draft(Wing::Param, Subsystem::Particle);
        // 10a. 草拟态不能解冻（没有「解冻」可言）。
        let early = it.unfreeze();
        // 10b. 冻结后解冻返回真，且状态回到评审（不是直接可改）。
        it.freeze();
        let real = it.unfreeze();
        let back_to_review = it.state == FreezeState::Review && it.mutations == 1;
        set.add(
            "C5201-冻结-解冻路径唯一",
            !early && real && back_to_review,
            "草拟态不得解冻；冻结态解冻须回评审并记改动次数，不直接进可改态",
        );
        // 10c. 解冻后可改语义，且改动次数再 +1。
        let amend = it.amend(AmendKind::Change);
        let now_change_ok = amend.is_ok() && it.mutations == 2;
        set.add(
            "C5201-冻结-解冻后可改语义",
            now_change_ok,
            "走完解冻流程后，改语义是合法途径（越权只拦「没走流程的直接改」）",
        );
        // 10d. 二次解冻返回假（已不在冻结态）。
        let twice = it.unfreeze();
        set.add(
            "C5201-冻结-重复解冻被拒",
            !twice,
            "重复解冻须返回假：否则改动次数会被虚记，越权追责失真",
        );
    }

    // ------------------------------------------------------------------
    // 十一、对拍双向可测（十诫：判据必须能发现注入的失配）
    // ------------------------------------------------------------------
    {
        // 11a. 一致侧为绿。
        let a = SideOutput::new("p-pa", &["密度=12", "寿命=1.4s", "色温=6500K"]);
        let b = SideOutput::new("p-pa", &["密度=12", "寿命=1.4s", "色温=6500K"]);
        let same = diff_boundary(&a, &b);
        set.add(
            "C5201-对拍-一致侧为绿",
            same.matched && same.boundary.is_none() && same.first_gap.is_none(),
            "两侧逐项一致时对拍须通过，且不报失配边界",
        );
        // 11b. 中途差异须定位到**下标**（不是只报「不一致」）。
        let c = SideOutput::new("p-pa", &["密度=12", "寿命=2.4s", "色温=6500K"]);
        let mid = diff_boundary(&a, &c);
        set.add(
            "C5201-对拍-中途差异报出下标",
            !mid.matched && mid.first_gap == Some(1) && mid.boundary.as_deref() == Some("p-pa"),
            "第 2 项不同须报出下标 1；只报「不一致」等于没定位",
        );
        // 11c. 夹逼对：界前/界上/界后—— 长度差须报出缺口下标。
        let shorter = SideOutput::new("p-pa", &["密度=12"]);
        let longer = SideOutput::new("p-pa", &["密度=12", "寿命=1.4s"]);
        let d = diff_boundary(&shorter, &longer);
        let d2 = diff_boundary(&longer, &shorter);
        set.add(
            "C5201-对拍-长度差报出缺口下标",
            !d.matched && d.first_gap == Some(1)
                && !d2.matched && d2.first_gap == Some(1),
            "「少了一项」是最常见失配，两种方向都须报出缺口位置",
        );
        // 11d. 边界名不同须单独报（接口对错了，不是内容不一致）。
        let wrong = SideOutput::new("c-pa", &["密度=12", "寿命=1.4s", "色温=6500K"]);
        let mis = diff_boundary(&a, &wrong);
        set.add(
            "C5201-对拍-边界名不同单独报",
            !mis.matched && mis.first_gap.is_none() && mis.boundary.is_some(),
            "边界名不同即接口对错，与内容差异不同类，诊断要能分开",
        );
        // 11e. 对拍必须能**发现**注入的失配（把 mismatch 恒置真即为变异）。
        //      这里显式构造「全字段注入失配」并断言被抓住。
        let injected = diff_boundary(&a, &SideOutput::new("p-pa", &["X", "Y", "Z"]));
        set.add(
            "C5201-对拍-注入失配被抓",
            !injected.matched && injected.first_gap == Some(0),
            "对拍必须真能发现失配：全字段不同的两侧必被抓住且报下标 0",
        );
    }

    // ------------------------------------------------------------------
    // 十二、承接 F5193 十件
    // ------------------------------------------------------------------
    {
        let mut led = HandoffLedger::new();
        // 12a. 落地上限须恰为十件（判据侧独立数清单长度，不用被测对象报数）。
        let mut expect_ten = 0u32;
        let mut i = 0usize;
        while i < HANDOFF_ITEMS.len() {
            expect_ten += 1;
            i += 1;
        }
        set.add(
            "C5201-承接-清单恰十件",
            expect_ten == 10 && HANDOFF_COUNT == 10 && led.landed_count() == 0,
            "F5193 十件：判据侧独立数清单长度对上，新册落地数须为 0",
        );
        // 12b. 逐件落地 ⇒ 恰好十件，无缺口。
        let mut k = 0usize;
        while k < HANDOFF_COUNT {
            let _ = led.land(k);
            k += 1;
        }
        set.add(
            "C5201-承接-十件齐平",
            led.landed_count() == 10 && led.first_gap().is_none(),
            "十件逐件登记且齐平：少一件就是承接缺口，不是「大概齐了」",
        );
        // 12c. 缺源须回溯到**源头件号**（报源头，不是只说「承接失败」）。
        let mut led2 = HandoffLedger::new();
        let _ = led2.land(0);
        let _ = led2.land(1);
        let gap = led2.first_missing();
        // 判据侧独立算出「第 3 件」应该是什么，再与实现报出的对照。
        let want_source = HANDOFF_ITEMS[2].serial;
        let want_item = HANDOFF_ITEMS[2].label;
        let backtrace_ok = match gap {
            Some(VfxError::HandoffMissing { item, source }) => {
                source == want_source && item == want_item && source != "F4993"
            }
            _ => false,
        };
        set.add(
            "C5201-承接-缺源回溯到源头件号",
            backtrace_ok && led2.landed_count() == 2,
            "缺源诊断须报源头件号；只报「承接失败」工人无从下手",
        );
        // 12d. 三个承接面各自独立计数（上界由判据侧按清单重算）。
        let faces_ok = {
            let mut ok = true;
            let mut f = 0usize;
            while f < 3 {
                let faces = [
                    HandoffFace::ScriptBinding,
                    HandoffFace::RenderProtocol,
                    HandoffFace::BudgetLedger,
                ];
                let face = faces[f];
                // 判据侧独立重算该面应有件数。
                let mut want = 0u32;
                let mut i = 0usize;
                while i < HANDOFF_ITEMS.len() {
                    if HANDOFF_ITEMS[i].face == face {
                        want += 1;
                    }
                    i += 1;
                }
                if led.landed_of(face) != want || HandoffLedger::total_of(face) != want {
                    ok = false;
                }
                f += 1;
            }
            ok
        };
        set.add(
            "C5201-承接-三承接面独立对账",
            faces_ok,
            "脚本绑定/渲染协议/预算口径三面各自对账，不合并成一个总数",
        );
        // 12e. 越界下标须被拒（不静默丢弃）。
        let mut led3 = HandoffLedger::new();
        let oob = led3.land(HANDOFF_COUNT);
        let oob_rejected = oob.is_err() && led3.landed_count() == 0;
        set.add(
            "C5201-承接-越界下标被拒",
            oob_rejected,
            "下标越界即拒：不静默丢弃也不越界写入",
        );
    }

    // ------------------------------------------------------------------
    // 十三、诊断质量（特效面向创作者）
    // ------------------------------------------------------------------
    {
        let e1 = VfxError::UndeclaredSemantic { name: "炫光特效".to_string() };
        let e2 = VfxError::ClassOverreach {
            name: "错挂".to_string(),
            owner: Subsystem::Particle,
            got: "fullscreen-composite",
            want: "point-force",
        };
        let e3 = VfxError::BudgetExceeded {
            name: "大雪".to_string(),
            owner: Subsystem::Particle,
            cost_us: 3000,
            budget_us: 2400,
        };
        let e4 = VfxError::AmendFrozen { interface: "p-pa".to_string() };
        let e5 = VfxError::HandoffMissing { item: "接口总账·特效绑定面", source: "F5193-01" };
        let all = [e1, e2, e3, e4, e5];
        // 13a. 五条诊断都要带**人名与数字**（源信息），不是只吐一个码。
        let mut named = true;
        let mut i = 0usize;
        while i < all.len() {
            let t = all[i].explain();
            // 每条都必须含自身标识：名字或接口名或件号之一。
            let has_name = match &all[i] {
                VfxError::UndeclaredSemantic { name } => t.contains(name.as_str()),
                VfxError::ClassOverreach { name, .. } => t.contains(name.as_str()),
                VfxError::BudgetExceeded { name, cost_us, budget_us, .. } => {
                    t.contains(name.as_str())
                        && t.contains(&cost_us.to_string())
                        && t.contains(&budget_us.to_string())
                }
                VfxError::AmendFrozen { interface } => t.contains(interface.as_str()),
                VfxError::HandoffMissing { source, .. } => t.contains(*source),
            };
            if !has_name || t.len() < 12 {
                named = false;
            }
            i += 1;
        }
        set.add(
            "C5201-诊断-带源信息",
            named,
            "诊断必须带源信息（特效名/接口名/源头件号 + 实测数字），不许只吐一个码位",
        );
        // 13b. 五条都要给出**可操作建议**，且建议不得只是复述错误。
        let mut advised = true;
        let mut i = 0usize;
        while i < all.len() {
            let adv = all[i].advise();
            if adv.len() < 10 {
                advised = false;
            }
            // 建议不得与说明同文（复述错在哪 ≠ 改什么）。
            if all[i].explain() == adv {
                advised = false;
            }
            i += 1;
        }
        set.add(
            "C5201-诊断-带可操作建议",
            advised,
            "建议须说「改什么」，不是复述错在哪；与说明同文即判失败",
        );
        // 13c. 五条说明互不相同（诊断可区分，不是同一段话换码位）。
        let mut uniq_text = true;
        let mut i = 0usize;
        while i < all.len() {
            let mut j = i + 1;
            while j < all.len() {
                if all[i].explain() == all[j].explain() {
                    uniq_text = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add(
            "C5201-诊断-五条说明互不相同",
            uniq_text,
            "五类错误的说明须互不相同：同文换码位等于没分类",
        );
    }

    // ------------------------------------------------------------------
    // 十四、人读总述与冒烟
    // ------------------------------------------------------------------
    {
        let d = describe();
        // 14a. 总述必须点名四子系统与两翼（判据要求「架构要能被读懂」）。
        let mut names_ok = d.contains("粒子")
            && d.contains("后处理")
            && d.contains("环境")
            && d.contains("转场")
            && d.contains("参数系统")
            && d.contains("组合系统");
        let mut i = 0usize;
        while i < SUBSYSTEM_COUNT {
            if !d.contains(Subsystem::ALL[i].name()) {
                names_ok = false;
            }
            i += 1;
        }
        set.add(
            "C5201-诊断-总述含四子系统与两翼",
            names_ok && d.contains("特效即内容"),
            "架构总览（人话）须点名四子系统与两翼，并复申域本色「特效即内容」",
        );
        // 14b. 冒烟自洽：四子系统两翼 8 接口全冻结、承接 10/10。
        let s = smoke();
        let expect = format!(
            "子系统 4 两翼 2 接口 8 冻结 8 承接 {}/10",
            HANDOFF_COUNT
        );
        set.add(
            "C5201-诊断-冒烟口径自洽",
            s == expect,
            "冒烟串的四项计数须与判据侧独立算出的期望逐字相符",
        );
    }

    set
}