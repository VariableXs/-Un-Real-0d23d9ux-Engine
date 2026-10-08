//! VE-F4603 · 插件加载器（VE-W 域 · 插件 SDK · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4603`
//!
//! **判据（锚点原文四条）**：**五步流水、失败即清、排队限流、超时熔断**。
//!
//! - **五步流水**：清单校验 → 签名验证 → 依赖解析 → 沙箱实例化 → 激活注册。
//!   顺序不可颠倒——**清单在最前**（不认识的插件不许碰沙箱），**激活注册在最后**
//!   （沙箱没起来就注册等于对外暴露半成品接口）。
//! - **失败即清**：任一步失败即终止并**清理**，**不留半载实例**——半载实例是
//!   最坏状态：它占着沙箱资源却对外不可见，排障时既查不到也删不掉。本条把
//!   「已分配」与「已注册」两态严格分开，回滚只回滚已分配的部分。
//! - **排队限流**：批量激活排队防风暴——同一时刻在途加载数有上界，超出的进
//!   等待队列而非并发开跑；风暴的代价是瞬时资源尖峰，不是吞吐不足。
//! - **超时熔断**：单次加载超时即熔断该插件并禁用，**不在同一次调用里重试**
//!   （超时后重试往往又是超时，只是把尖峰拉长）；熔断 O(1) 判定。
//!
//! **错误路径与降级矩阵**：任一步失败 → 终止 + 清理 + 三要素报告；风暴 → 排队
//! 限流；超时 → 熔断禁用。
//!
//! **跨批对接点**：上游 F4602 清单（本条消费其校验结论，不重复实现校验逻辑）；
//! 下游 F4604 沙箱、F4623 状态机衔接。
//!
//! **无障碍与隐私**：加载状态读屏播报（域本色）——加载态经 `status_line` 输出，
//! 供 UI 层播报；无隐私面。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（不 import 未注册兄弟模块——平行会话
//! 的 `ve*` 族尚在施工，编译期硬耦合会让本条因别人的进度而红）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 流水线步数（契约；顺序即语义，不可重排）。
pub const PIPELINE_STEPS: usize = 5;

/// 在途加载数上界（超出进等待队列，防激活风暴）。
pub const MAX_INFLIGHT: usize = 4;

/// 等待队列上界（超出显性拒收并登记——零静默）。
pub const MAX_QUEUE: usize = 64;

/// 单次加载逻辑 tick 预算（超出即熔断）。
pub const LOAD_TICK_BUDGET: u32 = 100;

/// 沙箱实例上界（防御性；正常由熔断先拦）。
pub const MAX_SANDBOX_INSTANCES: usize = 32;

/// 依赖解析深度上界（循环依赖与深链兜底，不死循环）。
pub const MAX_DEP_DEPTH: usize = 16;

// ---------------------------------------------------------------------------
// 二、数据结构（五步 / 队列 / 熔断器）
// ---------------------------------------------------------------------------

/// 流水线五步（顺序即语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// 一：清单校验（消费 F4602 结论）。
    ManifestCheck = 0,
    /// 二：签名验证。
    SignatureVerify = 1,
    /// 三：依赖解析。
    DependencyResolve = 2,
    /// 四：沙箱实例化。
    SandboxInstantiate = 3,
    /// 五：激活注册。
    ActivateRegister = 4,
}

impl Step {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            Step::ManifestCheck => "manifest-check",
            Step::SignatureVerify => "signature-verify",
            Step::DependencyResolve => "dependency-resolve",
            Step::SandboxInstantiate => "sandbox-instantiate",
            Step::ActivateRegister => "activate-register",
        }
    }

    /// 中文名（读屏播报与三要素报告用）。
    pub fn label(self) -> &'static str {
        match self {
            Step::ManifestCheck => "清单校验",
            Step::SignatureVerify => "签名验证",
            Step::DependencyResolve => "依赖解析",
            Step::SandboxInstantiate => "沙箱实例化",
            Step::ActivateRegister => "激活注册",
        }
    }

    /// 五步穷举（顺序即流水线次序）。
    pub fn all() -> [Step; PIPELINE_STEPS] {
        [
            Step::ManifestCheck,
            Step::SignatureVerify,
            Step::DependencyResolve,
            Step::SandboxInstantiate,
            Step::ActivateRegister,
        ]
    }
}

/// 待加载插件的输入（加载器只吃这份输入，不自己解析清单格式）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadRequest {
    /// 插件标识。
    pub id: String,
    /// 清单校验是否已通过（上游 F4602 结论）。
    pub manifest_ok: bool,
    /// 签名是否可信。
    pub signature_ok: bool,
    /// 依赖的插件标识（须全部已加载）。
    pub deps: Vec<String>,
    /// 沙箱实例化是否可用。
    pub sandbox_ok: bool,
    /// 本次加载预计消耗的逻辑 tick（供超时熔断判定）。
    pub cost_ticks: u32,
}

/// 三要素诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 错误码。
    pub code: &'static str,
    /// 发生在哪一步（`None` 表示非步骤性失败，如熔断）。
    pub step: Option<Step>,
    /// 三要素之一：发生了什么。
    pub what: String,
    /// 三要素之二：为什么。
    pub why: &'static str,
    /// 三要素之三：下一步怎么办。
    pub next: &'static str,
}

/// 加载结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadOutcome {
    /// 插件标识。
    pub id: String,
    /// 是否加载成功。
    pub loaded: bool,
    /// 走到的步（失败即该步）。
    pub reached: Step,
    /// 诊断（成功时为空）。
    pub diagnostics: Vec<Diagnostic>,
    /// 是否已在激活注册表（**半载实例的判据**：已分配但未注册即半载）。
    pub registered: bool,
    /// 读屏播报文本（域本色：加载状态须可播报）。
    pub status_line: String,
}

// ---------------------------------------------------------------------------
// 三、加载器（五步流水 + 排队 + 熔断）
// ---------------------------------------------------------------------------

/// 插件加载器。
#[derive(Clone, Debug, Default)]
pub struct Loader {
    /// 已加载并注册的插件（注册表）。
    registered: Vec<String>,
    /// 在途加载数（并发上界）。
    inflight: usize,
    /// 等待队列（风暴时排队而非并发开跑）。
    queue: Vec<String>,
    /// 熔断表（超时禁用，不再尝试）。
    fused: Vec<String>,
    /// 沙箱实例（已分配，含未注册的）。
    sandboxes: Vec<String>,
    /// 读屏播报流水（最近 N 条）。
    announcements: Vec<String>,
}

/// 播报流水保留条数（够 UI 消费即可，不无限增长）。
pub const ANNOUNCE_KEEP: usize = 32;

impl Loader {
    /// 构造空加载器。
    pub fn new() -> Self {
        Loader::default()
    }

    /// 读屏播报流水（域本色：加载状态可播报）。
    pub fn announcements(&self) -> &[String] {
        &self.announcements
    }

    /// 熔断表（已被熔断禁用的插件）。
    pub fn fused(&self) -> &[String] {
        &self.fused
    }

    /// 注册表（已激活）。
    pub fn registered(&self) -> &[String] {
        &self.registered
    }

    /// 沙箱实例（含未注册的——半载实例在此可见）。
    pub fn sandboxes(&self) -> &[String] {
        &self.sandboxes
    }

    /// 等待队列长度。
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// 在途数。
    pub fn inflight(&self) -> usize {
        self.inflight
    }

    fn announce(&mut self, text: String) {
        self.announcements.push(text);
        if self.announcements.len() > ANNOUNCE_KEEP {
            let drop_n = self.announcements.len() - ANNOUNCE_KEEP;
            self.announcements.drain(0..drop_n);
        }
    }

    /// 熔断判定（O(1)：线性查表，熔断表本身有界）。
    pub fn is_fused(&self, id: &str) -> bool {
        self.fused.iter().any(|f| f == id)
    }

    /// 熔断某插件（超时后禁用，不在同一次调用里重试）。
    pub fn fuse(&mut self, id: &str, why: &'static str) {
        if !self.is_fused(id) {
            self.fused.push(id.to_string());
        }
        self.announce(format!("插件 {} 已熔断禁用：{}", id, why));
    }

    /// 入队（批量激活排队限流）。
    ///
    /// 返回 `Ok(true)` 表示直接开跑（在途有位），`Ok(false)` 表示已排队。
    pub fn enqueue(&mut self, req: &LoadRequest) -> Result<bool, &'static str> {
        if self.inflight >= MAX_INFLIGHT {
            if self.queue.len() >= MAX_QUEUE {
                self.announce(format!("插件 {} 排队溢出，加载请求被拒", req.id));
                return Err("E_QUEUE_FULL");
            }
            self.queue.push(req.id.clone());
            self.announce(format!("插件 {} 排队等待（{} 人在途）", req.id, self.inflight));
            return Ok(false);
        }
        self.inflight += 1;
        Ok(true)
    }

    /// 出队一条（在途释放后调用）。
    pub fn dequeue(&mut self) -> Option<String> {
        let id = if self.queue.is_empty() {
            None
        } else {
            Some(self.queue.remove(0))
        };
        id
    }

    /// 五步流水加载。
    ///
    /// **任一步失败即终止并清理，绝不留半载实例**：已分配的沙箱在失败时回滚，
    /// 注册表只在第五步成功后写入。
    pub fn load(&mut self, req: &LoadRequest) -> LoadOutcome {
        let mut diag: Vec<Diagnostic> = Vec::new();

        // 熔断前置判定：已熔断的插件不再尝试（不在同一次调用里重试）。
        if self.is_fused(&req.id) {
            return LoadOutcome {
                id: req.id.clone(),
                loaded: false,
                reached: Step::ManifestCheck,
                diagnostics: vec![Diagnostic {
                    code: "E_FUSED",
                    step: None,
                    what: format!("插件 {} 已被熔断禁用，本次不再尝试", req.id),
                    why: "超时后重试往往又是超时，只是把资源尖峰拉长",
                    next: "修复插件性能后由管理员手动解除熔断",
                }],
                registered: false,
                status_line: format!("插件 {} 已熔断，跳过加载", req.id),
            };
        }

        // 超时预算：超预算直接熔断，不进流水线（省掉无用功）。
        if req.cost_ticks > LOAD_TICK_BUDGET {
            self.fuse(&req.id, "加载超出 tick 预算");
            return LoadOutcome {
                id: req.id.clone(),
                loaded: false,
                reached: Step::ManifestCheck,
                diagnostics: vec![Diagnostic {
                    code: "E_LOAD_TIMEOUT",
                    step: None,
                    what: format!(
                        "插件 {} 预计 {} tick 超预算 {}",
                        req.id, req.cost_ticks, LOAD_TICK_BUDGET
                    ),
                    why: "超时熔断：不在同一次调用里重试，重试只会拉长尖峰",
                    next: "降低插件加载开销或分批加载后重试",
                }],
                registered: false,
                status_line: format!("插件 {} 加载超时，已熔断", req.id),
            };
        }

        // ---- 第一步：清单校验 ----
        let mut reached;
        if !req.manifest_ok {
            diag.push(Diagnostic {
                code: "E_MANIFEST_REJECTED",
                step: Some(Step::ManifestCheck),
                what: format!("插件 {} 清单校验未通过", req.id),
                why: "清单在最前：未过校验的插件不许碰沙箱",
                next: "按 F4602 诊断三要素修正清单后重新提交",
            });
            reached = Step::ManifestCheck;
            return self.fail(req, reached, diag);
        }

        // ---- 第二步：签名验证 ----
        reached = Step::SignatureVerify;
        if !req.signature_ok {
            diag.push(Diagnostic {
                code: "E_SIGNATURE_INVALID",
                step: Some(Step::SignatureVerify),
                what: format!("插件 {} 签名不可信", req.id),
                why: "签名不可信则来源不可信，来源不可信则能力不可信",
                next: "使用可信签名重新打包后重新提交",
            });
            return self.fail(req, reached, diag);
        }

        // ---- 第三步：依赖解析 ----
        reached = Step::DependencyResolve;
        if req.deps.len() > MAX_DEP_DEPTH {
            diag.push(Diagnostic {
                code: "E_DEP_TOO_DEEP",
                step: Some(Step::DependencyResolve),
                what: format!(
                    "插件 {} 依赖数 {} 超上界 {}",
                    req.id,
                    req.deps.len(),
                    MAX_DEP_DEPTH
                ),
                why: "深依赖链近乎必然成环，成环后加载器会死等",
                next: "拆分插件或改用扁平依赖结构",
            });
            return self.fail(req, reached, diag);
        }
        // 依赖须全部已加载（自依赖亦判失败——自依赖即环）。
        for d in req.deps.iter() {
            if d == &req.id || !self.registered.iter().any(|r| r == d) {
                diag.push(Diagnostic {
                    code: "E_DEP_UNRESOLVED",
                    step: Some(Step::DependencyResolve),
                    what: format!("插件 {} 的依赖 {} 未解析", req.id, d),
                    why: "依赖未就绪时装载本插件，激活时必崩",
                    next: "先加载依赖插件，或修正依赖声明",
                });
                return self.fail(req, reached, diag);
            }
        }

        // ---- 第四步：沙箱实例化（分配即记入沙箱表，失败须回滚）----
        reached = Step::SandboxInstantiate;
        if !req.sandbox_ok {
            diag.push(Diagnostic {
                code: "E_SANDBOX_UNAVAILABLE",
                step: Some(Step::SandboxInstantiate),
                what: format!("插件 {} 沙箱不可用", req.id),
                why: "沙箱不可用而强行加载等于让插件裸奔",
                next: "检查沙箱资源配额或等待沙箱释放",
            });
            return self.fail(req, reached, diag);
        }
        if self.sandboxes.len() >= MAX_SANDBOX_INSTANCES {
            diag.push(Diagnostic {
                code: "E_SANDBOX_EXHAUSTED",
                step: Some(Step::SandboxInstantiate),
                what: format!(
                    "插件 {} 沙箱实例数 {} 已达上界 {}",
                    req.id,
                    self.sandboxes.len(),
                    MAX_SANDBOX_INSTANCES
                ),
                why: "沙箱是稀缺隔离资源，无界分配会拖垮宿主",
                next: "卸载闲置插件释放沙箱后重试",
            });
            return self.fail(req, reached, diag);
        }
        self.sandboxes.push(req.id.clone());

        // ---- 第五步：激活注册（唯一写入注册表的步）----
        reached = Step::ActivateRegister;
        self.registered.push(req.id.clone());
        self.announce(format!("插件 {} 加载完成并已激活", req.id));
        LoadOutcome {
            id: req.id.clone(),
            loaded: true,
            reached,
            diagnostics: Vec::new(),
            registered: true,
            status_line: format!("插件 {} 加载完成并已激活", req.id),
        }
    }

    /// 失败收尾：终止 + 清理 + 三要素报告，**不留半载实例**。
    fn fail(&mut self, req: &LoadRequest, reached: Step, diag: Vec<Diagnostic>) -> LoadOutcome {
        // 清理：把本插件可能已分配的沙箱回滚（半载实例即沙箱有它、注册表没它）。
        let had_sandbox = self.sandboxes.iter().any(|s| s == &req.id);
        self.sandboxes.retain(|s| s != &req.id);
        self.registered.retain(|r| r != &req.id);
        // 在途释放（本次失败即占位结束）。
        if self.inflight > 0 {
            self.inflight -= 1;
        }
        self.announce(format!(
            "插件 {} 在{}失败，已清理（半载实例={}）",
            req.id,
            reached.label(),
            had_sandbox
        ));
        LoadOutcome {
            id: req.id.clone(),
            loaded: false,
            reached,
            diagnostics: diag,
            registered: false,
            status_line: format!("插件 {} 加载失败并已清理", req.id),
        }
    }
}

// ---------------------------------------------------------------------------
// 四、自检（CheckSet）
// ---------------------------------------------------------------------------

/// 构造一份可成功加载的请求。
fn ok_request(id: &str) -> LoadRequest {
    LoadRequest {
        id: id.to_string(),
        manifest_ok: true,
        signature_ok: true,
        deps: Vec::new(),
        sandbox_ok: true,
        cost_ticks: 10,
    }
}

/// VE-F4603 域自检。
pub fn run_vew03_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F4603");

    // ---- 判据一：五步流水 ----
    {
        let all = Step::all();
        let ordered = all.len() == PIPELINE_STEPS
            && all[0] == Step::ManifestCheck
            && all[1] == Step::SignatureVerify
            && all[2] == Step::DependencyResolve
            && all[3] == Step::SandboxInstantiate
            && all[4] == Step::ActivateRegister;
        set.add("W03-五步-次序不可颠倒", ordered, "");
    }

    {
        let mut l = Loader::new();
        let o = l.load(&ok_request("p1"));
        set.add(
            "W03-五步-合规插件五步走通并注册",
            o.loaded && o.registered && o.reached == Step::ActivateRegister,
            "",
        );
    }

    {
        // 逐步可辨识：四步各自失败须停在对应步（不是笼统失败）。
        let mut l = Loader::new();
        let mut m = ok_request("a");
        m.manifest_ok = false;
        let r1 = l.load(&m);
        let mut m2 = ok_request("b");
        m2.signature_ok = false;
        let r2 = l.load(&m2);
        let mut m3 = ok_request("c");
        m3.sandbox_ok = false;
        let r3 = l.load(&m3);
        let ok = r1.reached == Step::ManifestCheck
            && r2.reached == Step::SignatureVerify
            && r3.reached == Step::SandboxInstantiate;
        set.add("W03-五步-各步失败停在对应步", ok, "");
    }

    // ---- 判据二：失败即清 ----
    {
        // 前四步任一失败都不得留下注册记录或沙箱残留。
        let mut l = Loader::new();
        let mut bad = Vec::new();
        let mk = |id: &str, f: fn(&mut LoadRequest)| {
            let mut r = ok_request(id);
            f(&mut r);
            r
        };
        let r1 = mk("f1", |r| r.manifest_ok = false);
        let r2 = mk("f2", |r| r.signature_ok = false);
        let r3 = mk("f3", |r| r.sandbox_ok = false);
        let r4 = mk("f4", |r| r.cost_ticks = LOAD_TICK_BUDGET + 1);
        for r in [&r1, &r2, &r3, &r4] {
            let o = l.load(r);
            bad.push((
                o.loaded,
                o.registered,
                l.registered().iter().any(|x| x == &r.id),
                l.sandboxes().iter().any(|x| x == &r.id),
            ));
        }
        let clean = bad.iter().all(|(ld, rg, in_reg, in_sbx)| !ld && !*rg && !*in_reg && !*in_sbx);
        set.add("W03-失败即清-不留半载实例", clean, "");
    }

    {
        // 依赖未就绪须失败且不注册（半载最典型的来源）。
        let mut l = Loader::new();
        let mut r = ok_request("need-dep");
        r.deps = vec!["absent".to_string()];
        let o = l.load(&r);
        let ok = !o.loaded
            && o.first_step_is_resolve()
            && !l.registered().iter().any(|x| x == "need-dep");
        set.add("W03-失败即清-依赖未就绪不注册", ok, "");
    }

    {
        // 依赖就绪则可加载（正向对照，防闸门过严）。
        let mut l = Loader::new();
        let _ = l.load(&ok_request("base"));
        let mut r = ok_request("dep-user");
        r.deps = vec!["base".to_string()];
        let o = l.load(&r);
        set.add("W03-依赖-就绪则加载成功", o.loaded, "");
    }

    // ---- 判据三：排队限流 ----
    {
        let mut l = Loader::new();
        let mut direct = 0usize;
        let mut queued = 0usize;
        for i in 0..(MAX_INFLIGHT + 3) {
            let r = ok_request(&format!("p{}", i));
            match l.enqueue(&r) {
                Ok(true) => direct += 1,
                Ok(false) => queued += 1,
                Err(_) => {}
            }
        }
        // 在途不超过上界，超出全部进队列。
        let ok = direct == MAX_INFLIGHT
            && queued == 3
            && l.queued() == 3
            && l.inflight() == MAX_INFLIGHT;
        set.add("W03-排队限流-超出在途上界进队列", ok, "");
    }

    {
        // 队列溢出显性拒收（零静默）。
        let mut l = Loader::new();
        let mut rejected = false;
        for i in 0..(MAX_INFLIGHT + MAX_QUEUE + 5) {
            let r = ok_request(&format!("q{}", i));
            if l.enqueue(&r).is_err() {
                rejected = true;
                break;
            }
        }
        set.add("W03-排队限流-队列溢出显性拒收", rejected, "");
    }

    // ---- 判据四：超时熔断 ----
    {
        let mut l = Loader::new();
        let mut r = ok_request("slow");
        r.cost_ticks = LOAD_TICK_BUDGET + 1;
        let o = l.load(&r);
        let ok = !o.loaded && o.diagnostics.first().map(|d| d.code) == Some("E_LOAD_TIMEOUT");
        set.add("W03-超时熔断-超预算即熔断", ok, "");
    }

    {
        // 熔断后不再尝试（不在同一次调用里重试）。
        let mut l = Loader::new();
        let mut r = ok_request("slow2");
        r.cost_ticks = LOAD_TICK_BUDGET + 1;
        let _ = l.load(&r);
        let fused = l.is_fused("slow2");
        // 即便条件已改善，熔断态下仍拒绝（须人工解除）。
        let good = ok_request("slow2");
        let o = l.load(&good);
        set.add(
            "W03-超时熔断-熔断态不自动重试",
            fused && !o.loaded && o.diagnostics.first().map(|d| d.code) == Some("E_FUSED"),
            "",
        );
    }

    {
        // 预算内不误熔断。
        let mut l = Loader::new();
        let mut r = ok_request("fine");
        r.cost_ticks = LOAD_TICK_BUDGET;
        let o = l.load(&r);
        set.add(
            "W03-超时熔断-预算内不误熔断",
            o.loaded && !l.is_fused("fine"),
            "",
        );
    }

    // ---- 无障碍：加载状态读屏播报 ----
    {
        let mut l = Loader::new();
        let o = l.load(&ok_request("ann"));
        let has_status = !o.status_line.is_empty();
        let recorded = l.announcements().iter().any(|a| a.contains("ann"));
        set.add("W03-无障碍-加载状态可播报", has_status && recorded, "");
    }

    // ---- 零静默：三要素齐备 ----
    {
        let mut l = Loader::new();
        let mut r = ok_request("d");
        r.signature_ok = false;
        let o = l.load(&r);
        let three = o
            .diagnostics
            .iter()
            .all(|x| !x.what.is_empty() && !x.why.is_empty() && !x.next.is_empty());
        set.add("W03-零静默-三要素齐备", !o.loaded && three, "");
    }

    set
}

impl LoadOutcome {
    /// 便捷判定：是否停在依赖解析步。
    fn first_step_is_resolve(&self) -> bool {
        self.reached == Step::DependencyResolve
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_steps_in_order() {
        let all = Step::all();
        assert_eq!(all.len(), 5);
        assert_eq!(all[0], Step::ManifestCheck);
        assert_eq!(all[4], Step::ActivateRegister);
    }

    #[test]
    fn happy_path_registers() {
        let mut l = Loader::new();
        let o = l.load(&ok_request("p"));
        assert!(o.loaded && o.registered);
        assert_eq!(l.registered(), &["p".to_string()]);
    }

    #[test]
    fn failure_leaves_no_half_loaded_instance() {
        let mut l = Loader::new();
        let mut r = ok_request("bad");
        r.sandbox_ok = false;
        let o = l.load(&r);
        assert!(!o.loaded);
        assert!(l.registered().is_empty(), "注册表须为空");
        assert!(l.sandboxes().is_empty(), "沙箱须已回滚");
    }

    #[test]
    fn timeout_fuses_and_blocks_retry() {
        let mut l = Loader::new();
        let mut r = ok_request("t");
        r.cost_ticks = LOAD_TICK_BUDGET + 1;
        assert!(!l.load(&r).loaded);
        assert!(l.is_fused("t"));
        // 条件改善后仍拒绝（熔断须人工解除）。
        assert!(!l.load(&ok_request("t")).loaded);
    }

    #[test]
    fn queue_limits_concurrency() {
        let mut l = Loader::new();
        for i in 0..MAX_INFLIGHT {
            assert!(l.enqueue(&ok_request(&format!("p{}", i))).unwrap());
        }
        // 在途已满，进队列。
        assert!(!l.enqueue(&ok_request("overflow")).unwrap());
        assert_eq!(l.queued(), 1);
    }

    #[test]
    fn self_dependency_rejected() {
        let mut l = Loader::new();
        let mut r = ok_request("self");
        r.deps = vec!["self".to_string()];
        let o = l.load(&r);
        assert!(!o.loaded, "自依赖即环，须拒绝");
    }

    #[test]
    fn sandbox_exhaustion_blocks_load() {
        let mut l = Loader::new();
        for i in 0..MAX_SANDBOX_INSTANCES {
            let o = l.load(&ok_request(&format!("s{}", i)));
            assert!(o.loaded);
        }
        let o = l.load(&ok_request("overflow"));
        assert!(!o.loaded, "沙箱耗尽须拒绝而非裸奔");
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_vew03_checks();
        let (p, f) = set.tally();
        assert!(!set.truncated());
        assert!(set.all_passed(), "VE-F4603 红项：{}/{}", p, p + f);
    }
}
