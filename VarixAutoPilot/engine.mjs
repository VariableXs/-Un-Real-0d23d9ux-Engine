/**
 * VarixAutoPilot · 自动化引擎
 *
 * 职责：把一段提示词**填进** WorkBuddy 的输入框，**点发送**，并给出
 *       客观的成功/失败判定。
 *
 * 三条铁律（都是实踩出来的，不是设计洁癖）：
 * 1. **发送中不许发**：按钮class 带 `--sending` 时它的语义是「停止」，
 *    点下去会**中断对方正在跑的活**。这不是「发送失败」，是「不该发」。
 * 2. **默认不真发**：`dryRun` 为真时只填入不点发送。
 * 3. **成功判定靠双重证据**：固定 sleep + 单点采样会误报
 *    （见下方 sendPrompt 的时序注释）。
 *
 * 填入方式说明：输入框是 **Slate.js 富文本编辑器**，
 * Playwright 的 `locator.type()` 对它无效（它接管 contenteditable，
 * 不产生 Playwright 期望的键盘事件），必须走 CDP 的 `Input.insertText`。
 */

import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

import { CDP_ENDPOINT, probe } from './collect.mjs';

/** 截图存证目录。 */
export const SHOT_DIR = join(process.cwd(), 'shots');

/** 时间戳（文件名用）。 */
function stamp() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return (
    `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}_` +
    `${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`
  );
}

const say = (m) => console.log(`[ap] ${m}`);

/** 取渲染页（排除 devtools 页）。 */
async function renderPage(browser) {
  const ctx = browser.contexts()[0];
  if (!ctx) throw new Error('没有浏览器上下文：CDP 连上了但拿不到 context');
  const page = ctx.pages().find((p) => !p.url().startsWith('devtools'));
  if (!page) throw new Error('找不到渲染页：CDP 通了但没有可用 page');
  return page;
}

/** 「新建任务」按钮：靠文字定位（所有 tab 共用同一个 class）。 */
const NEW_TASK_SELECTORS = [
  'button.conversation-list-tab-button',
];
const NEW_TASK_TEXT = '新建任务';

/** 输入框选择器。 */
const EDITOR_SELECTOR = 'div[data-slate-editor="true"][contenteditable="true"]';

/** 发送键选择器：排除 sending 态（那个是「停止」）。 */
const SEND_SELECTOR = 'button.cr-send-button:not(.cr-send-button--sending)';

/**
 * 打开一个新对话。
 * @returns {Promise<boolean>} 是否点到了「新建任务」
 */
export async function newConversation(browser) {
  const page = await renderPage(browser);
  for (const sel of NEW_TASK_SELECTORS) {
    const btns = page.locator(sel);
    const n = await btns.count();
    for (let i = 0; i < n; i += 1) {
      const t = ((await btns.nth(i).innerText()) || '').trim();
      if (t === NEW_TASK_TEXT) {
        await btns.nth(i).click();
        await page.waitForTimeout(1200);
        say('已点击「新建任务」，等待新对话输入框就绪…');
        return true;
      }
    }
  }
  throw new Error(
    `找不到「${NEW_TASK_TEXT}」按钮（试了 ${NEW_TASK_SELECTORS.join('、')}）。` +
      '可能是 WorkBuddy 改了 class 名——跑 --list 看真实结构再改选择器。'
  );
}

/**
 * 把文本填进输入框。
 * @returns {Promise<{ok: boolean, chars: number, why?: string}>}
 */
export async function fillPrompt(browser, text) {
  const page = await renderPage(browser);
  const ed = page.locator(EDITOR_SELECTOR).first();
  if ((await ed.count()) === 0) {
    return { ok: false, chars: 0, why: `找不到输入框（${EDITOR_SELECTOR}）` };
  }
  const box = await ed.boundingBox();
  if (!box || box.width < 40 || box.height < 18) {
    return { ok: false, chars: 0, why: '输入框不可见（尺寸异常），可能对话区在滚动位置不对' };
  }
  await ed.click();
  // 清空再插（复用上一轮残留会串内容）
  await page.keyboard.press('Control+A');
  await page.keyboard.press('Delete');
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Input.insertText', { text });
  await page.waitForTimeout(400);
  // 回读校验：静默失败必须暴露
  const got = await page.evaluate((sel) => {
    const e = document.querySelector(sel);
    return e ? (e.innerText || '').trim().length : -1;
  }, EDITOR_SELECTOR);
  if (got < Math.min(50, text.length)) {
    const fail = join(SHOT_DIR, `${stamp()}_fillfail.png`);
    try {
      mkdirSync(SHOT_DIR, { recursive: true });
      await page.screenshot({ path: fail });
    } catch { /* 截图失败不掩盖主错误 */ }
    return { ok: false, chars: got, why: `回读校验失败：期望 ≥${Math.min(50, text.length)} 字符，实得 ${got}；截图 ${fail}` };
  }
  return { ok: true, chars: got };
}

/**
 * 点发送，并用双重证据判定成功与否。
 *
 * ★ 为什么不能「睡 2 秒回读编辑器长度」（真缺陷，勿改回去）★
 * 点击发送后 Slate 的时序是：**先把内容渲染成消息节点 → 再清空编辑器**。
 * 2 秒时点常卡在中间，回读到的是还没清完的旧内容 ⇒ 误报「发送存疑」，
 * 而实际消息已发出（截图与 DOM 双证）。
 * 正解：轮询最多 `timeoutMs`，两个证据任一成立即算成功——
 * 编辑器已清空，**或**消息已出现在对话流（与编辑器状态无关，更强）。
 */
export async function clickSend(browser, opts = {}) {
  const timeoutMs = opts.timeoutMs || 8000;
  const probeText = opts.probeText || '';
  const page = await renderPage(browser);

  const btn = page.locator(SEND_SELECTOR).first();
  if ((await btn.count()) === 0) {
    const cls = await page.evaluate(() => {
      const b = document.querySelector('button.cr-send-button');
      return b ? String(b.className) : '(无发送键)';
    });
    if (/--sending/.test(cls)) {
      const e = new Error('对方正在生成中（发送键是「停止」），已拒绝发送以免中断其工作');
      e.code = 'BUSY';
      throw e;
    }
    throw new Error(`找不到就绪态发送键（${SEND_SELECTOR}）；实际 class="${cls}"。可能是版本变了，跑 --list核对。`);
  }
  await btn.click();
  say('已点击发送按钮');

  let sent = false;
  let evidence = '';
  for (let i = 0; i < Math.ceil(timeoutMs / 1000); i += 1) {
    await page.waitForTimeout(1000);
    const st = await page.evaluate(
      ({ sel, needle }) => {
        const e = document.querySelector(sel);
        const editorChars = e ? (e.innerText || '').trim().length : -1;
        let inStream = false;
        if (needle) {
          const msgs = [
            ...document.querySelectorAll(
              '[data-message-author-role="user"], .cr-user-message, [class*="user-message"]'
            ),
          ];
          inStream = msgs.some((m) => (m.innerText || '').includes(needle.slice(0, 40)));
        }
        return { editorChars, inStream };
      },
      { sel: EDITOR_SELECTOR, needle: probeText }
    );
    if (st.inStream) {
      sent = true;
      evidence = `消息已出现在对话流（编辑器残留 ${st.editorChars} 字符，属渲染滞后）`;
      break;
    }
    if (st.editorChars >= 0 && st.editorChars < 50) {
      sent = true;
      evidence = `输入框已清空（剩余 ${st.editorChars} 字符）`;
      break;
    }
  }
  return { sent, evidence, timeoutMs };
}

/**
 * 查忙闲。忙 = 对方正在生成。
 * @returns {Promise<{sending: boolean, label: string}>}
 */
export async function busyState(browser) {
  const page = await renderPage(browser);
  return page.evaluate(() => {
    const b = document.querySelector('button.cr-send-button');
    const cls = b ? (typeof b.className === 'string' ? b.className : '') : '';
    return {
      sending: /--sending/.test(cls),
      label: b ? (b.getAttribute('aria-label') || '') : '(无发送键)',
    };
  });
}

/**
 * 一键全流程：可选开新对话 → 填入 → （可选）发送。
 *
 * @param {import('playwright-core').Browser} browser
 * @param {{text: string, dryRun?: boolean, openNew?: boolean, timeoutMs?: number}} o
 */
export async function runFlow(browser, o) {
  const text = String(o.text || '');
  if (!text.trim()) throw new Error('提示词为空：拒绝发送空内容（会让下一轮 AI 无从下手）');
  const dryRun = o.dryRun !== false;

  mkdirSync(SHOT_DIR, { recursive: true });
  const page = await renderPage(browser);
  const before = join(SHOT_DIR, `${stamp()}_before.png`);
  await page.screenshot({ path: before });

  // ★ busy 检查必须放在**填入之前**，不只是发送前 ★
  //
  // 早先的顺序是「填入 → 检查 busy → 发」，看起来覆盖到了，实际有个洞：
  // 填入那一刻若对方正在生成，内容会先进编辑器；随后检查发现忙就放弃，
  // 于是输入框里**残留着一段没人认领的文字**——下一轮接手的人（或用户）
  // 会看到一段不明来源的提示词，还以为是对方写的。这是"隐异常"。
  //
  // 正解：忙就**在动手之前**退出，连一个字都不留。
  //干跑模式不做此限制——干跑的 purpose 就是在忙碌时也能预演填入效果。
  if (!dryRun) {
    const b0 = await busyState(browser);
    if (b0.sending) {
      const e = new Error(
        `对方正在生成中（发送键是「${b0.label || '停止'}」），本轮未执行任何写入。` +
          `理由：忙时发送要么被拒、要么误点「停止」打断对方的工作。` +
          `等空闲再点发送即可。`
      );
      e.code = 'BUSY';
      throw e;
    }
  }

  if (o.openNew) await newConversation(browser);

  const filled = await fillPrompt(browser, text);
  if (!filled.ok) throw new Error(`填入失败：${filled.why}`);

  const filledShot = join(SHOT_DIR, `${stamp()}_filled.png`);
  await page.screenshot({ path: filledShot });

  if (dryRun) {
    say('DRY-RUN：内容已就位，未发送。确认无误后加 --send。');
    return { ok: true, dryRun: true, chars: filled.chars, shot: filledShot, before };
  }

  // 填入这段时间里对方可能已开始生成 ⇒ 二次检查
  const pre = await busyState(browser);
  if (pre.sending) {
    const e = new Error(
      `填入期间对方开始生成（发送键变「${pre.label || '停止'}」），已放弃发送。` +
        `内容仍留在输入框（${filled.chars} 字符），等空闲再点发送即可。`
    );
    e.code = 'BUSY';
    throw e;
  }

  const r = await clickSend(browser, { timeoutMs: o.timeoutMs || 8000, probeText: text });
  const after = join(SHOT_DIR, `${stamp()}_sent.png`);
  await page.screenshot({ path: after });
  return { ok: r.sent, dryRun: false, chars: filled.chars, evidence: r.evidence, shot: after, before };
}

export { CDP_ENDPOINT, probe };
