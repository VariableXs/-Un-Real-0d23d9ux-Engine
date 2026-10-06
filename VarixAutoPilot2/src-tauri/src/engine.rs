//! 引擎：填入 + 发送 + 忙闲保护。
//!
//! 这里复刻了 Node 版实测踩出的 6 个坑，每条都有代价：
//!
//! 1. **可见性不能用 Playwright 的 isVisible**——Electron 的 `file://` 页恒false。
//!    Rust 侧无 Playwright，但同类陷阱是「拿不到真实布局」，
//!    所以统一在页面内自己算 `getBoundingClientRect()`。
//! 2. **输入框是 Slate.js 富文本编辑器**，它接管 contenteditable，
//!    必须走 CDP 的 `Input.insertText`，不能用 `Input.dispatchKeyEvent` 逐字打。
//! 3. **发送键 busy 态是 `--sending`**，此时它的语义是「停止」；
//!    点下去会**打断对方正在跑的活**。必须排除。
//! 4. **尺寸门槛要按调用方分设**：输入框 200×40，发送键 32×32。
//!    早先统一用 40×18 门槛，把 32×32 的发送键挡掉了。
//! 5. **发送成功判定不能「睡 2 秒回读」**：Slate 时序是
//!    「先渲染消息 → 再清空编辑器」，固定 sleep 会卡在中间误报。
//!    正解：轮询 + 双重证据（编辑器清空 **或** 消息入流）。
//! 6. **忙时要在动手之前退出，且一个字都不留**。
//!    早先「先填后检」，忙时放弃会留一段孤儿文字在输入框，
//!    下一轮接手的人会以为那是对方写的。

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use log;
use std::time::{Duration, Instant};

use crate::cdp::Cdp;

/// 忙闲状态。
#[derive(Debug, Clone)]
pub struct Busy {
    pub sending: bool,
    pub label: String,
}

/// ★★ 空闲判定用「三条独立判据取多数」★★
///
/// 实测（tools/idle_probe.py 8 次采样，各判据完全一致）：
///   ① 发送键 class 无 --sending/--stop
///   ③ 独立停止按钮数：忙=1 / 闲=0
///   ④ 生成动画元素数：忙=3 / 闲=0
///   ⑦ 末条助手消息字数：忙时=6（"正在生成"）
///
/// 为什么不用单条：WorkBuddy 改版时某一条可能失效，
/// 三条同时失效的概率低得多。**任一条说忙即视为忙**（保守——
/// 宁可多等一轮，绝不在对方忙碌时误发打断他）。
#[derive(Debug, Clone)]
pub struct IdleVerdict {
    pub idle: bool,
    /// 三条判据各自的结论，便于排查（哪条失效能看出来）
    pub by_btn: bool,
    pub by_stop_btn: bool,
    pub by_anim: bool,
    /// 判定依据摘要（给界面看，不裸奔）
    pub reason: String,
}

/// ★ 点「停止」，打断对方当前的生成 ★
///
/// 定位依据（来自 `idle_verdict` 的实测）：
///   `button.cr-send-button` 的 class 含 `--stop` 时，
///   它的语义就是「停止」—— 这也是我们判定「忙」的依据之一。
///
/// 为什么要专门写这个：Variable 明确要求「不要等，直接停止然后发」
/// （2026-10-06）。之前把「不打断对方」当成红线，于是永远在等，
/// 而等待没有上限也没有进度 —— 正是他抱怨的「老是这样没反应」。
///
/// 用 CDP 原生鼠标事件而非页面内 `.click()`：
/// 后者在这个 WebView2 里不触发 onclick（本项目已实测多次）。
pub async fn stop_generation(cdp: &Cdp) -> Result<bool> {
    // ① 先定位：找得到就拿它的中心坐标
    let hit = cdp
        .eval(
            r#"(() => {
              const cands = Array.from(document.querySelectorAll(
                'button.cr-send-button[class*="stop"],button[class*="stop"],' +
                'button[aria-label*="停止"],button[title*="停止"]'
              )).filter((e) => {
                const r = e.getBoundingClientRect();
                return r.width > 8 && r.height > 8;   // 过滤不可见的
              });
              if (!cands.length) return { found: false };
              const b = cands[0];
              const r = b.getBoundingClientRect();
              return {
                found: true,
                x: Math.round(r.left + r.width / 2),
                y: Math.round(r.top + r.height / 2),
                label: (b.getAttribute('aria-label') || b.innerText || '').trim().slice(0, 20),
              };
            })"#,
        )
        .await?;
    let found = hit.get("found").and_then(|v| v.as_bool()).unwrap_or(false);
    if !found {
        return Ok(false);   // 找不到 ⇒ 很可能它本来就没在跑
    }
    let x = hit.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
    let y = hit.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
    let label = hit
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // ② CDP 原生点击（mousePressed + mouseReleased 必须成对）
    cdp.call(
        "Input.dispatchMouseEvent",
        json!({
            "type": "mouseMoved",
            "x": x,
            "y": y,
            "button": "none",
        }),
    )
    .await?;
    cdp.call(
        "Input.dispatchMouseEvent",
        json!({
            "type": "mousePressed",
            "x": x,
            "y": y,
            "button": "left",
            "clickCount": 1,
        }),
    )
    .await?;
    // 30~60ms 是真实人类点击的间隔，太短会被当成双击或被忽略
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    cdp.call(
        "Input.dispatchMouseEvent",
        json!({
            "type": "mouseReleased",
            "x": x,
            "y": y,
            "button": "left",
            "clickCount": 1,
        }),
    )
    .await?;

    log::info!("[stop] 已点停止（{label}）@ {x},{y}");
    Ok(true)
}

/// 空闲判定（三判据多数一致）。
pub async fn idle_verdict(cdp: &Cdp) -> Result<IdleVerdict> {
    let v = cdp
        .eval(
            r#"(() => {
              const btn = document.querySelector('button.cr-send-button');
              const cls = btn ? String(btn.className || '') : '';
              const stopBtns = document.querySelectorAll(
                'button[class*="stop"],button[aria-label*="停止"],[title*="停止"]'
              ).length;
              // ★★★ 动画判据必须排除假阳性 ★★★
              //
              // 实测踩到：对面上常驻一个
              //   <span class="cr-message-list__top-loading-spinner">  13x13
              // 它是**对话列表顶部的加载指示器**，与「对方是否在生成」无关，
              // 却会命中 [class*="loading-"] ⇒ anims 恒 ≥1
              // ⇒ by_anim 恒为忙 ⇒ 三判据凑不出 2/3 空闲
              // ⇒ **worker 永远等，永远不发**（用户实测：等了 65 次仍不发）
              //
              // 三重过滤，缺一不可：
              // ① 区域：只看消息区，不要全页面
              // ② 尺寸：13x13 的装饰图标不算，真正的流式输出是大块文本
              // ③ 黑名单：显式排除已知的列表指示器
              const animSel = [
                '[class*="streaming"]', '[class*="generating"]',
                '[class*="typing"]', '[class*="loading-"]',
              ].join(',');
              const anims = Array.from(document.querySelectorAll(animSel)).filter((e) => {
                // ① 区域：必须在消息列表或主内容区里
                const inRegion = e.closest(
                  '[class*="message-list"],[class*="message_list"],main,[role="main"]'
                ) !== null;
                if (!inRegion) return false;
                // ② 尺寸：真正的流式输出是大块文本容器
                const r = e.getBoundingClientRect();
                if (r.width < 200 || r.height < 24) return false;
                // ③ 黑名单：列表顶部的加载指示器
                if (/top-loading-spinner|list__.*loading/.test(String(e.className || ''))) {
                  return false;
                }
                return true;
              }).length;
              return {
                by_btn: btn ? !/--sending|--stop/.test(cls) : false,
                by_stop_btn: stopBtns === 0,
                by_anim: anims === 0,
                label: btn ? (btn.getAttribute('aria-label') || '') : '(无发送键)',
                stopBtns: stopBtns,
                anims: anims,
              };
            })"#,
        )
        .await?;
    let by_btn = v.get("by_btn").and_then(|x| x.as_bool()).unwrap_or(false);
    let by_stop_btn = v.get("by_stop_btn").and_then(|x| x.as_bool()).unwrap_or(false);
    let by_anim = v.get("by_anim").and_then(|x| x.as_bool()).unwrap_or(false);
    // 多数一致（2/3 即空闲）；全否视为忙
    let votes = [by_btn, by_stop_btn, by_anim];
    let idle_count = votes.iter().filter(|x| **x).count();
    let idle = idle_count >= 2;
    let reason = format!(
        "发送键{}·停止键{}·动画{}（{}/3 判空闲）",
        if by_btn { "闲" } else { "忙" },
        if by_stop_btn { "闲" } else { "忙" },
        if by_anim { "闲" } else { "忙" },
        idle_count
    );
    Ok(IdleVerdict {
        idle,
        by_btn,
        by_stop_btn,
        by_anim,
        reason,
    })
}

/// 执行结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct FlowResult {
    pub ok: bool,
    pub dry_run: bool,
    pub chars: usize,
    pub evidence: String,
    /// 忙时拒绝的说明（空=未拒绝）
    pub busy_reason: String,
}

/// 错误分类：让上层能区分「忙」与「坏了」。
#[derive(Debug)]
pub enum EngineError {
    /// 对方正在生成，不该发
    Busy(String),
    /// 真故障
    Other(anyhow::Error),
}

impl From<anyhow::Error> for EngineError {
    fn from(e: anyhow::Error) -> Self {
        EngineError::Other(e)
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Busy(m) => write!(f, "BUSY: {m}"),
            EngineError::Other(e) => write!(f, "{e}"),
        }
    }
}

/// ★ 编辑器真实字数★
///
/// ★★ 为什么不能用 innerText / textContent ★★
/// Slate 的空输入框在 DOM 里长这样（实测 innerHTML 逐字比对）：
///   <span data-slate-leaf="true">
///     <br>
///     <span data-slate-placeholder="true" contenteditable="false" ...>
///       今天帮你做些什么？@添加上下文，/调用技能与指令
///     </span>
///   </span>
/// 关键：placeholder 是**真实存在的子元素**，
/// 所以 innerText 与 textContent **都会把提示文字算进去**（实测都是 25）。
///
/// 后果很严重（都实测踩过）：
/// - 空输入框被算成 25 字 ⇒「填入是否成功」回读校验恒不准
/// - 「编辑器已清空」判据永不成立 ⇒ **误报发送失败**（就是用户说的
///   「发送功能无法运行」）
/// - 若拿它做备份再写回 ⇒把提示固化成真实文字，**污染用户输入框**
///
/// 正解：克隆一份 DOM，删掉 [data-slate-placeholder] 及其内容，再数长度。
pub const EDITOR_CHARS_JS: &str = r#"(() => {
  const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  if (!e) return -1;
  const c = e.cloneNode(true);
  c.querySelectorAll('[data-slate-placeholder]').forEach(n => n.remove());
  return (c.textContent || '').trim().length;
})"#;

/// 查忙闲。

/// ★ 保证在「助理」页且输入框可用 ★
///
/// ★★ 为什么必须有这个守卫 ★★
/// 实测：WorkBuddy 的侧栏有7 个标签（助理 / 项目 / 专家·技能·连接器 /
/// 定时任务 / 资料库 / 更多），**只有「助理」页有输入框**。
/// 在「专家」页时实测：编辑器 0 个、发送键 0 个、
/// placeholder 0 个 —— 此时执行 fill_prompt 会**静默失败**
///（eval 返回 0 或null，不抛错，看起来像填了但没反应）。
///
/// 而 skill 探测会切到专家页 —— 如果用户探测完不切回来，
/// 后续所有填入/发送都会静默失效。所以这一步是必需的。
pub async fn ensure_assist_page(cdp: &Cdp) -> Result<()> {
    let v = cdp
        .eval(
            r#"(() => {
      const active = Array.from(document.querySelectorAll('.conversation-list-tab-button'))
        .filter(b => /active/.test(String(b.className)))
        .map(b => (b.innerText || '').trim());
      return {
        tab: active.length ? active[0] : '',
        hasEditor: !!document.querySelector('div[data-slate-editor=\"true\"][contenteditable=\"true\"]'),
        hasSend: document.querySelectorAll('button.cr-send-button').length > 0,
      };
    })()"#,
        )
        .await?;
    let tab = v.get("tab").and_then(|x| x.as_str()).unwrap_or("");
    let has_editor = v.get("hasEditor").and_then(|x| x.as_bool()).unwrap_or(false);
    let has_send = v.get("hasSend").and_then(|x| x.as_bool()).unwrap_or(false);
    if has_editor && has_send {
        return Ok(());
    }
    // 切回「助理」
    log::warn!("当前标签 {tab}（输入框就绪={has_editor}）→ 切回「助理」");
    cdp.eval(
        r#"(() => {
      for (const b of document.querySelectorAll('.conversation-list-tab-button')) {
        if ((b.innerText || '').trim() === '助理') { b.click(); return true; }
      }
      return false;
    })()"#,
    )
    .await?;
    // 轮询等就绪（切页是异步渲染，编辑器要等一会儿才挂载）
    for i in 0..14 {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let ok = cdp
            .eval(
                r#"(() => {
          const e = document.querySelector('div[data-slate-editor=\"true\"][contenteditable=\"true\"]');
          const b = document.querySelectorAll('button.cr-send-button').length;
          return !!(e && b > 0);
        })()"#,
            )
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
        if i == 13 {
            bail!("切回「助理」页后输入框仍未就绪：可能 WorkBuddy 界面结构变了，或当前处于其它状态");
        }
    }
    Ok(())
}

pub async fn busy_state(cdp: &Cdp) -> Result<Busy> {
    let v = cdp
        .eval(
            r#"(() => {
              const b = document.querySelector('button.cr-send-button');
              const cls = b ? ((typeof b.className==='string')?b.className:'') : '';
              // ★★ 与 idle_verdict 的 by_btn 判据完全一致 ★★
              // 实测踩到的矛盾（用户截图）：徽章「空闲」vs 报错「正在生成中」。
              // 原因：这里只判 --sending，闸门还判 --stop 与动画。
              // --stop 时（正在生成）徽章说空闲、闸门说忙 ⇒ 用户以为按钮坏了。
              // 闸门是对的（发送键此刻语义是「停止」，点了会打断对方），
              // 所以只能让徽章迁就闸门，绝不能放宽闸门。
              // ★ 与 idle_verdict 的动画判据完全一致（含三重过滤）★★
              // 见idle_verdict 处的详细注释—— 那个 13x13 的
              // top-loading-spinner 曾让 anims 恒 ≥1，害得worker 永不发。
              const animSel = [
                '[class*="streaming"]', '[class*="generating"]',
                '[class*="typing"]', '[class*="loading-"]',
              ].join(',');
              const anims = Array.from(document.querySelectorAll(animSel)).filter((e) => {
                if (e.closest(
                  '[class*="message-list"],[class*="message_list"],main,[role="main"]'
                ) === null) return false;
                const r = e.getBoundingClientRect();
                if (r.width < 200 || r.height < 24) return false;
                if (/top-loading-spinner|list__.*loading/.test(String(e.className || ''))) {
                  return false;
                }
                return true;
              }).length;
              return { sending: /--sending|--stop/.test(cls) || anims > 0,
                       label: b ? (b.getAttribute('aria-label')||'') : '(无发送键)' };
            })()"#,
        )
        .await?;
    Ok(Busy {
        sending: v.get("sending").and_then(|x| x.as_bool()).unwrap_or(false),
        label: v
            .get("label")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// 点「新建任务」。
async fn new_conversation(cdp: &Cdp) -> Result<()> {
    // ★ 靠文字定位 ★
    // 所有 tab（助理/项目/专家/定时/资料库）共用 class `conversation-list-tab-button`，
    // 按class 选会命中第一个（那是「新建任务」之外的项）。所以逐个比 innerText。
    let v = cdp
        .eval(
            r#"(() => {
              const btns = Array.from(document.querySelectorAll('button.conversation-list-tab-button'));
              for (const b of btns) {
                if ((b.innerText||'').trim() === '新建任务') { b.click(); return true; }
              }
              return false;
            })()"#,
        )
        .await?;
    let clicked = v.as_bool().unwrap_or(false);
    if !clicked {
        bail!(
            "找不到「新建任务」按钮。可能是 WorkBuddy 改了 class 名——\
             跑node probe.mjs --list 核对真实结构后修改本文件的选择器。"
        );
    }
    tokio::time::sleep(Duration::from_millis(1200)).await;
    Ok(())
}

/// 把文本填进输入框。
///
/// ★★ 忙时必须先等 ★★
/// 实测：WorkBuddy 生成中时，Slate 编辑器**不接受** `Input.insertText`，
/// 甚至可能被临时卸载 ⇒ 回读拿到 -1 ⇒ 报「回读校验失败」。
/// 根因不是「界面没就绪」，而是**没等空闲**——
/// 明明有 `idle_verdict`，此前这里压根没调用它。
async fn fill_prompt(cdp: &Cdp, text: &str) -> Result<usize> {
    // ① 忙闲闸门：等空闲（最多 20 秒）
    //
    // 为何必须等：发送键在忙时的语义是「停止」，
    // 此时点它会打断用户正在跑的活——所以这里既等、又绝不代劳停止。
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut waited = 0u64;
        loop {
            match idle_verdict(cdp).await {
                Ok(v) if v.idle => break,
                Ok(v) => {
                    if std::time::Instant::now() >= deadline {
                        // ★ 这里用 bail!（而不是 EngineError::Busy）★
                        // 因为 fill_prompt 返回 anyhow::Result，类型上只能 bail。
                        // 「要不要标 busy」由 run_flow 层判定——那里才是边界。
                        bail!(
                            "对方正在生成中，等了 {waited} 秒还没空闲。\
                             填入已取消（**没有打断它**）。"
                        );
                    }
                    if waited % 5 == 0 {
                        log::info!("[fill] 等空闲：{}", v.reason);
                    }
                    waited += 2;
                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                }
                Err(e) => {
                    if std::time::Instant::now() >= deadline {
                        bail!("读忙闲状态失败，等了 {waited} 秒：{e}");
                    }
                    waited += 2;
                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                }
            }
        }
        if waited > 0 {
            log::info!("[fill]等到空闲（等了 {} 秒）", waited);
        }
    }

    // ② 可见性判定：页面内自己算（坑 1）
    let vis = cdp
        .eval(
            r#"(() => {
              const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
              if (!e) return { found: false, ok: false, w: 0, h: 0 };
              const r = e.getBoundingClientRect();
              return { found: true, ok: r.width > 200 && r.height > 40, w: Math.round(r.width), h: Math.round(r.height) };
            })()"#,
        )
        .await?;
    let found = vis.get("found").and_then(|v| v.as_bool()).unwrap_or(false);
    if !found {
        // ★ 把「编辑器为什么不在」说清楚 ★
        // 实测两种根因完全不同：
        //   (a) 对话还没打开 / 页面在加载
        //   (b) 忙时 Slate 临时卸载编辑器
        // 不分清就会一律报「界面没就绪」，把责任推给用户。
        let why = cdp
            .eval(
                r#"(() => {
                  const btn = document.querySelector('button.cr-send-button');
                  const cls = btn ? String(btn.className || '') : '';
                  return {
                    busy: /--sending|--stop/.test(cls),
                    has_send: !!btn,
                    body: (document.body ? document.body.innerText.length : 0),
                    url: location.href.slice(-40),
                  };
                })()"#,
            )
            .await
            .unwrap_or(serde_json::Value::Null);
        let busy = why.get("busy").and_then(|v| v.as_bool()).unwrap_or(false);
        let has_send = why.get("has_send").and_then(|v| v.as_bool()).unwrap_or(false);
        let body = why.get("body").and_then(|v| v.as_i64()).unwrap_or(0);
        if busy {
            bail!(
                "找不到输入框——但页面**正忙**（发送键是「停止」）。\n\
                 对方还在生成，编辑器暂不可用。"
            );
        }
        if !has_send || body < 50 {
            bail!(
                "找不到输入框（div[data-slate-editor]），而且页面内容极少（{body} 字）。\
                 多半是这个对话**还没打开**，或WorkBuddy 还在启动。\n\
                 先在 WorkBuddy 里打开一个能输入的对话，再回来点。"
            );
        }
        bail!(
            "找不到输入框（div[data-slate-editor]）。\n\
             页面有内容（{body} 字）且不忙，说明编辑器换了实现\
             （WorkBuddy 升级过）。需要更新这个选择器。"
        );
    }
    let ok = vis.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if !ok {
        let w = vis.get("w").and_then(|v| v.as_i64()).unwrap_or(0);
        let h = vis.get("h").and_then(|v| v.as_i64()).unwrap_or(0);
        bail!("输入框不可见（实测 {w}×{h}，需 >200×40）。可能对话区滚动位置不对。");
    }

    // 聚焦 + 清空（复用上一轮残留会串内容）
    cdp.eval(
        r#"(() => {
          const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
          if (e) { e.focus(); e.click(); }
          return true;
        })()"#,
    )
    .await?;
    // 全选删除：走 CDP 原生按键（坑 2：不能用逐字dispatchKeyEvent）
    for key in [("ControlLeft", "a"), ("Delete", "")] {
        let r = cdp
            .call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": if key.1.is_empty() { "keyDown" } else { "keyDown" },
                    "key": key.1,
                    "code": key.0,
                    "modifiers": if key.1 == "a" { 2 } else { 0 },
                    "windowsVirtualKeyCode": if key.1 == "a" { 65 } else { 46 },
                }),
            )
            .await;
        if r.is_err() {
            // 清空失败不算致命：insertText 会追加，而我们会回读校验
            break;
        }
        let _ = cdp
            .call(
                "Input.dispatchKeyEvent",
                json!({ "type": "keyUp", "key": key.1, "code": key.0 }),
            )
            .await;
    }

    // ★ 填入：Input.insertText（坑 2 的正解）★
    cdp.call(
        "Input.insertText",
        json!({ "text": text }),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(400)).await;

    // 回读校验：静默失败必须暴露
    let got = cdp.eval(EDITOR_CHARS_JS).await?;
    let n = got.as_i64().unwrap_or(-1);
    let expect = std::cmp::min(50, text.chars().count() as i64);
    if n < expect {
        // ★ 补上「当时忙不忙」——这是本次实测最关键的信息 ★
        let v2 = idle_verdict(cdp).await.ok();
        let busy_s = match &v2 {
            Some(v) if !v.idle => format!("（当时仍在**生成中**：{}）", v.reason),
            Some(v) => format!("（当时**空闲**：{}）", v.reason),
            None => "（读不到忙闲状态）".to_string(),
        };
        if n < 0 {
            bail!(
                "填入后读不到编辑器内容（实得 {n}），{busy_s}\n\
                 写入可能没生效。WorkBuddy 升级换了编辑器实现时也会这样。"
            );
        }
        bail!(
            "回读校验失败：期望 ≥{expect} 字符，实得 {n}。{busy_s}\n\
             写入被截断或部分生效。可能是超长文本、含特殊字符，\
             或编辑器对 insertText 有长度限制。"
        );
    }
    Ok(n.max(0) as usize)
}

/// 点发送，并用双重证据判定。
async fn click_send(cdp: &Cdp, probe_text: &str, timeout: Duration) -> Result<(bool, String)> {
    // 排除 busy 态（坑 3）
    let hit = cdp
        .eval(
            r#"(() => {
              // ★ 必须同时排除 --sending 与 --stop ★
              // 实测：生成中该按钮 class 是
              //   cr-send-button cr-send-button--sending cr-send-button--stop
              // 只排除 --sending 的话，--stop 状态仍会被选中⇒ 点下去是「停止」
              // ⇒ 打断对方正在跑的活。
              const b = document.querySelector(
                'button.cr-send-button:not(.cr-send-button--sending):not(.cr-send-button--stop)'
              );
              if (!b) {
                const any = document.querySelector('button.cr-send-button');
                return { found: false, cls: any ? String(any.className) : '(无发送键)' };
              }
              b.click();
              return { found: true, cls: '' };
            })()"#,
        )
        .await?;
    if !hit.get("found").and_then(|v| v.as_bool()).unwrap_or(false) {
        let cls = hit.get("cls").and_then(|v| v.as_str()).unwrap_or("");
        if cls.contains("--sending") {
            bail!("对方正在生成中（发送键是「停止」），已拒绝发送以免打断其工作");
        }
        bail!("找不到就绪态发送键；实际 class=\"{cls}\"。可能是版本变了。");
    }

    // ★ 轮询 + 双重证据（坑 5）★
    let needle: String = probe_text.chars().take(40).collect();
    let start = Instant::now();
    let mut last = String::new();
    while start.elapsed() < timeout {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let st = cdp
            .eval(&format!(
                r#"(() => {{
                  const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
                  // ★ 排除 placeholder 的真实字数 ★（理由见 EDITOR_CHARS_JS）
                  const editorChars = (() => {{
                    if (!e) return -1;
                    const c = e.cloneNode(true);
                    c.querySelectorAll('[data-slate-placeholder]').forEach(n => n.remove());
                    return (c.textContent || '').trim().length;
                  }})();
                  let inStream = false;
                  const nd = {needle:?};
                  if (nd) {{
                    const msgs = Array.from(document.querySelectorAll(
                      '[data-message-author-role="user"], .cr-user-message, [class*="user-message"]'));
                    inStream = msgs.some(m => (m.innerText||'').includes(nd));
                  }}
                  return {{ editorChars: editorChars, inStream: inStream }};
                }})()"#
                // `{needle:?}` 已按序隐式捕获变量，末尾不能再传参
            ))
            .await?;
        let chars = st.get("editorChars").and_then(|v| v.as_i64()).unwrap_or(-1);
        let in_stream = st.get("inStream").and_then(|v| v.as_bool()).unwrap_or(false);
        last = format!("编辑器 {chars} 字符，入流={in_stream}");
        if in_stream {
            return Ok((
                true,
                format!("消息已出现在对话流（编辑器残留 {chars} 字符，属渲染滞后）"),
            ));
        }
        if (0..50).contains(&chars) {
            return Ok((true, format!("输入框已清空（剩余 {chars} 字符）")));
        }
    }
    Ok((false, format!("8 秒内未确认发送（{last}）")))
}

/// 全流程：可选开新对话 → 填入 → （可选）发送。
/// 发一条到指定对话（队列/循环 worker 用）。
///
/// 与 run_flow 的区别：
/// - run_flow 是「用户手动点」语义，会拒绝忙（返回 Busy错误给界面弹）
/// - send_only 是「worker 内部」语义，忙时返回 Err(Anyhow)，由 worker 决定重试
///   ——界面不该被每轮失败弹窗刷屏。
pub async fn send_only(cdp: &Cdp, text: &str, conv_id: &str) -> Result<String> {
    if text.trim().is_empty() {
        bail!("待发内容为空：拒绝发送");
    }
    // ★ 必须在助理页，否则填入会静默失败（实测：专家页无输入框）★
    ensure_assist_page(cdp).await?;
    // 忙时一个字都不写进输入框（坑 6）
    let v = idle_verdict(cdp).await?;
    if !v.idle {
        bail!("对方忙（{}），本轮未写入任何内容", v.reason);
    }
    // 切到目标对话（conv_id 非空时）
    if !conv_id.trim().is_empty() {
        switch_conversation(cdp, conv_id).await?;
        // 切换后界面会重渲染，稍等一下再填
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
    let n = fill_prompt(cdp, text).await?;
    // 填入期间可能已开始生成 ⇒ 二次检查（防止填了但没发，留孤儿文字）
    let v2 = idle_verdict(cdp).await?;
    if !v2.idle {
        // 清理：只删不插（不备份-写回）
        clear_editor(cdp).await?;
        bail!("填入期间对方开始生成（{}），已清理输入框", v2.reason);
    }
    let (ok, ev) = click_send(cdp, text, Duration::from_secs(8)).await?;
    if !ok {
        clear_editor(cdp).await?;
        bail!("点发送后未确认成功：{ev}");
    }
    Ok(format!("{n} 字符 · {ev}"))
}

/// 切换到指定对话（点侧栏项）。
///
/// ★ 为什么不能用 `el.click()` ★（2026-10-06 实测，D3/D9 同源）
/// React 合成事件对脚本派发的裸 `.click()` 不响应——会话项点了没反应、
/// 视图不切换，随后 fill_prompt 会把内容填进**当前打开的别的会话**。
/// 正解（tools/dispatch_tower.py 同款、已实测）：点内层 `_card_` 元素，
/// 派发完整 MouseEvent 序列，并轮询确认选中态真的落在了目标会话上。
/// 首次点击偶发被吞（D9），所以每轮都重派发，直到确认或超时。
async fn switch_conversation(cdp: &Cdp, conv_id: &str) -> Result<()> {
    // ① 派发点击（mouseover→…→click，坐标取内层卡片左上角附近）
    let click_js = format!(
        r#"((cid) => {{
          const el = document.querySelector(
            'div.conversation-item[data-conversation-id="' + cid + '"]');
          if (!el) return {{ found: false }};
          const node = el.querySelector('[class*="_card_"]')
            || el.firstElementChild || el;
          const r = node.getBoundingClientRect();
          const o = {{ bubbles: true, cancelable: true, view: window,
            clientX: r.left + 8, clientY: r.top + 8, button: 0, detail: 1 }};
          ['mouseover','mousemove','mousedown','mouseup','click']
            .forEach((t) => node.dispatchEvent(new MouseEvent(t, o)));
          return {{ found: true }};
        }})({})"#,
        serde_json::to_string(conv_id)
            .unwrap_or_else(|_| String::from("\"\""))
    );
    // ② 选中态确认：选中标记在侧栏项**后代**的 `_selected_` 类上
    //   （不在侧栏项自身——实测踩过，D2）
    let active_js = r#"(() => {
      for (const e of document.querySelectorAll('div.conversation-item')) {
        const hit = Array.from(e.querySelectorAll('*')).find(
          (k) => /_selected_/.test(String(k.className || '')));
        if (hit) return e.getAttribute('data-conversation-id') || '';
      }
      return '';
    })()"#;

    for round in 0..8 {
        let hit = cdp.eval(&click_js).await?;
        if !hit.get("found").and_then(|v| v.as_bool()).unwrap_or(false) {
            bail!("侧栏找不到该对话（可能已被关闭）");
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        let active = cdp.eval(active_js).await?;
        if active.as_str() == Some(conv_id) {
            if round > 0 {
                log::info!("[switch] 第 {} 次点击才生效（首次被吞，D9）", round + 1);
            }
            return Ok(());
        }
    }
    bail!("切换后 3 秒内未确认选中态落在目标会话上，放弃（防填错会话）")
}

/// 清空输入框（★ 只删不插 ★）。
///
/// 绝不用「备份 innerText 再写回」——实测那会把 Slate 的 placeholder
/// 「今天帮你做些什么？…」固化成真实文字（25 → 51 字符），污染用户输入框。
/// 详见 EDITOR_CHARS_JS 上方的注释。
pub async fn clear_editor(cdp: &Cdp) -> Result<()> {
    cdp.eval(
        r#"(() => {
      const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
      if (!e) return 0;
      e.focus();
      const s = window.getSelection();
      const r = document.createRange();
      r.selectNodeContents(e);
      s.removeAllRanges(); s.addRange(r);
      document.execCommand('delete');
      return 1;
    })()"#,
    )
    .await?;
    Ok(())
}

pub async fn run_flow(
    cdp: &Cdp,
    text: &str,
    dry_run: bool,
    open_new: bool,
) -> std::result::Result<FlowResult, EngineError> {
    if text.trim().is_empty() {
        return Err(EngineError::Other(anyhow!(
            "提示词为空：拒绝发送空内容（会让下一轮 AI 无从下手）"
        )));
    }
    let _chars = text.chars().count();

    // ★ 必须在助理页，否则填入会静默失败 ★
    ensure_assist_page(cdp).await?;

    // ★ 忙时在动手之前退出（坑 6）★
    // 干跑不做此限制——干跑的意义就是忙碌时也能预演填入效果。
    if !dry_run {
        let b = busy_state(cdp).await?;
        if b.sending {
            return Err(EngineError::Busy(format!(
                "对方正在生成中（发送键是「{}」），本轮未执行任何写入。\
                 理由：忙时发送要么被拒、要么误点「停止」打断对方的工作。\
                 等空闲再点发送即可。",
                if b.label.is_empty() { "停止" } else { &b.label }
            )));
        }
    }

    if open_new {
        new_conversation(cdp).await?;
    }

    // ★★ 干跑必须还原输入框 ★★
    // 实测踩到：干跑把编辑器从「今天帮你做些什么？」的 25 字符占位
    // 换成了探针文本 14 字符 —— 干跑本该"什么都不留下"。
    // 真实场景更糟：用户手打了半句话还没发，干跑会直接吃掉它。
    //
    // 做法：填入前把原内容存下来（连同"是不是占位提示"一起判），
    // 干跑完原样写回。占位提示本身就等同于"空"，写回它没有副作用。
    // ★★ 忙闲翻译层（本轮关键修复）★★
    // `fill_prompt` 返回 anyhow::Error，里面混着「忙」与「真故障」两类。
    // 而前端的自动降级入队**只看 `err.busy`**：
    //     if (r.err?.busy && real) { 自动入队 }
    // 所以必须在这里把「忙」挑出来升级成 EngineError::Busy，
    // 否则降级逻辑永不触发（实测踩过：报错一字未变）。
    //
    // 判据：文案里出现「正在生成中」/「正忙」/「还没空闲」/「等空闲」。
    // 为什么不改 fill_prompt 直接返回 EngineError：
    //   它的签名是 anyhow::Result<usize>，改了会牵连 dry_run 的还原路径；
    //   而 run_flow 才是面向前端的边界，翻译放这里最合适。
    let n = match fill_prompt(cdp, text).await {
        Ok(n) => n,
        Err(e) => {
            let msg = format!("{e:#}");
            let busy_hint = ["正在生成中", "正忙", "还没空闲", "等空闲", "发送键是「停止」"]
                .iter()
                .any(|k| msg.contains(k));
            return Err(if busy_hint {
                EngineError::Busy(msg)
            } else {
                EngineError::Other(anyhow::anyhow!(msg))
            });
        }
    };

    if dry_run {
        // ★ 还原必须「只删不插」★
        //
        // 踩过的坑（实测）：早先备份 innerText 再原样写回，
        // 结果把占位提示「今天帮你做些什么？@ 添加上下文，/调用技能与指令」
        // 变成了**真实输入文字**（25 → 51 字符，两份叠在一起）。
        //
        // 根因：Slate 编辑器空着时，`innerText` 会把 **placeholder 文本**读出来。
        // 它不是用户输入，是渲染层的假内容。
        // 于是"备份-写回"把假内容固化成真内容——**污染了用户的输入框**。
        //
        // 正解：空输入框 → 一律 **清空**（execCommand('delete')），
        // 绝不 insertText回占位提示。清空后 placeholder 会自动回来，
        // 且那仍然是假内容，零污染。
        let restore = r#"(() => {
          const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
          if (!e) return 0;
          e.focus();
          const sel = window.getSelection();
          const r = document.createRange();
          r.selectNodeContents(e);
          sel.removeAllRanges(); sel.addRange(r);
          document.execCommand('delete');   // ★ 只删，不插 ★
          return (e.innerText || '').trim().length;
        })()"#;
        let restored = cdp.eval(restore).await.unwrap_or(Value::Null);
        // ★ 校验清空确实生效：静默失败必须暴露（SOUL：异常零静默）
        let left = restored.as_i64().unwrap_or(-1);
        if left > 0 {
            log::warn!("干跑清空后仍剩 {left} 字符（占位提示不算字数，若>0 说明有残留）");
        } else {
            log::info!("干跑已清空输入框（还原为占位态）");
        }
        return Ok(FlowResult {
            ok: true,
            dry_run: true,
            chars: n,
            evidence: format!("已预览 {n} 字符并还原输入框，未发送"),
            busy_reason: String::new(),
        });
    }

    // 填入这段时间里对方可能已开始生成 ⇒ 二次检查
    let b = busy_state(cdp).await?;
    if b.sending {
        return Err(EngineError::Busy(format!(
            "填入期间对方开始生成（发送键变「{}」），已放弃发送。\
             内容仍留在输入框（{n} 字符），等空闲再点发送即可。",
            if b.label.is_empty() { "停止" } else { &b.label }
        )));
    }

    let (sent, evidence) = click_send(cdp, text, Duration::from_secs(8)).await?;
    Ok(FlowResult {
        ok: sent,
        dry_run: false,
        chars: n,
        evidence,
        busy_reason: String::new(),
    })
}
