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
use serde_json::json;
use std::time::{Duration, Instant};

use crate::cdp::Cdp;

/// 忙闲状态。
#[derive(Debug, Clone)]
pub struct Busy {
    pub sending: bool,
    pub label: String,
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

/// 查忙闲。
pub async fn busy_state(cdp: &Cdp) -> Result<Busy> {
    let v = cdp
        .eval(
            r#"(() => {
              const b = document.querySelector('button.cr-send-button');
              const cls = b ? ((typeof b.className==='string')?b.className:'') : '';
              return { sending: /--sending/.test(cls),
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
async fn fill_prompt(cdp: &Cdp, text: &str) -> Result<usize> {
    // 可见性判定：页面内自己算（坑 1）
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
        bail!("找不到输入框（div[data-slate-editor]）");
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
    let got = cdp
        .eval(
            r#"(() => {
              const e = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
              return e ? (e.innerText||'').trim().length : -1;
            })()"#,
        )
        .await?;
    let n = got.as_i64().unwrap_or(-1);
    let expect = std::cmp::min(50, text.chars().count() as i64);
    if n < expect {
        bail!(
            "回读校验失败：期望 ≥{expect} 字符，实得 {n}。\
             界面可能没就绪，或编辑器换了实现。"
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
              const b = document.querySelector('button.cr-send-button:not(.cr-send-button--sending)');
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
                  const editorChars = e ? (e.innerText||'').trim().length : -1;
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

    let n = fill_prompt(cdp, text).await?;

    if dry_run {
        return Ok(FlowResult {
            ok: true,
            dry_run: true,
            chars: n,
            evidence: "内容已就位，未发送".to_string(),
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
