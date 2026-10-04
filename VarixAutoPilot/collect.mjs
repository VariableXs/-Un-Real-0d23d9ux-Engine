/**
 * VarixAutoPilot · 数据采集层
 *
 * 三源合一（全部实测可读，不是推演）：
 *   ① 对话列表  → CDP `div.conversation-item`（标题 + 相对时间 + 选中态）
 *   ②模型       → CDP `button.cr-model-selector__trigger[title]` 与各对话消耗条
 *   ③ 工作目录  → `~/.workbuddy/sessions/*.json` 的 `cwd` 字段
 *
 * 为什么工作目录不也从 CDP 读：实测 DOM 里**没有**任何 data-cwd /
 * data-workspace 属性，标题属性里也没有路径。硬编一个选择器只会得到空值。
 * sessions/*.json 是客户端自己的持久化，`cwd` 字段就是会话启动目录，
 * 那是权威来源。
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

/** CDP 端点（与 WorkBuddy 启动参数一致）。 */
export const CDP_ENDPOINT = process.env.AUTOPILOT_CDP || 'http://127.0.0.1:9222';

/** 会话元数据目录。 */
const SESSIONS_DIR = join(homedir(), '.workbuddy', 'sessions');

// ---------------------------------------------------------------------------
// 一、工作目录：读 sessions/*.json
// ---------------------------------------------------------------------------

/**
 * 读全部会话的 `cwd`，建 **sessionId → 元数据** 索引。
 *
 * ★ 关键实测结论（决定了整套配对逻辑）★
 * DOM 里的 `div.conversation-item` 带 `data-conversation-id` 属性，
 * 而该值**就是** sessions/*.json 里的 `sessionId`（三个样本逐一验证过：
 * a09df76e… → 29316.json、6a70cd4d… → 46896.json、e042f2d0… → 14956.json）。
 * 所以工作目录可以**精确配对**，不需要按时间顺序瞎猜。
 *
 * 早先的版本按「最近更新的会话」顺序对齐，结果当前对话被配到
 * `_attic/2026-10-04-ve-产线/cdp` —— 明显错误（那个目录只是我跑脚本的地方，
 * 不是对话的工作目录）。**顺序对齐在多会话并行时必然错配**，
 * 而错配出来的路径比没有路径更危险：它看起来是真的。
 *
 * @returns {Map<string, {cwd: string, startedAt: number, updatedAt: number, kind: string}>}
 */
export function readSessionCwds() {
  const out = new Map();
  let files;
  try {
    files = readdirSync(SESSIONS_DIR).filter((f) => f.endsWith('.json'));
  } catch {
    return out;   // 目录不存在时如实返回空，不抛
  }
  for (const f of files) {
    const p = join(SESSIONS_DIR, f);
    try {
      const d = JSON.parse(readFileSync(p, 'utf8'));
      if (!d || typeof d.cwd !== 'string' || !d.cwd) continue;
      const sid = String(d.sessionId || f.replace(/\.json$/, ''));
      out.set(sid, {
        cwd: d.cwd,
        startedAt: Number(d.startedAt) || 0,
        updatedAt: Number(d.updatedAt) || safeMtime(p),
        kind: String(d.kind || ''),
      });
    } catch {
      // 单个会话文件损坏（可能正被写）不该拖垮整体采集——跳过，
      // 上层会看到该对话的 cwd 为空并如实标「未查到」
    }
  }
  return out;
}

/** 取 mtime，失败返回 0（不让一个坏文件中断整轮）。 */
function safeMtime(p) {
  try {
    return statSync(p).mtimeMs;
  } catch {
    return 0;
  }
}

/**
 * 按 cwd 归组会话：想知道「哪些对话在同一个工程里干活」时用。
 * @returns {Map<string, number>} cwd → 会话数
 */
export function groupByCwd(cwds) {
  const m = new Map();
  for (const v of cwds.values()) m.set(v.cwd, (m.get(v.cwd) || 0) + 1);
  return m;
}

// ---------------------------------------------------------------------------
// 二、CDP 侧：对话列表 / 模型 / 发送态
// ---------------------------------------------------------------------------

/**
 * 页面内求值：一次性把能拿的都拿，避免多次 evaluate 往返。
 * 全部在浏览器上下文里跑，返回纯 JSON（不能返回 DOM 节点）。
 */
const PAGE_PROBE = () => {
  const txt = (e) => (e && (e.innerText || e.textContent) || '').replace(/\s+/g, ' ').trim();

  // ① 对话项
  const items = [...document.querySelectorAll('div.conversation-item')];
  const convs = items.map((e, idx) => {
    const r = e.getBoundingClientRect();
    const cls = typeof e.className === 'string' ? e.className : '';
    const selected = /active|selected|current/i.test(cls)
      || !!e.querySelector('[class*="active"],[class*="selected"]');

    //★ 配对键：实测该属性值 === sessions/*.json 的 sessionId ★
    const convId = e.getAttribute('data-conversation-id') || '';

    // 标题：对话项内部是「标题 + 相对时间」两段（用 \n 分隔）
    const raw = (e.innerText || '').replace(/\r/g, '');
    const lines = raw.split('\n').map((x) => x.trim()).filter(Boolean);
    // 末行若形如"7小时前 / 14分钟前"则是相对时间
    let relTime = '';
    let titleLines = lines;
    const last = lines[lines.length - 1] || '';
    if (/^\d+\s*(秒|分钟|小时|天|周)前$/.test(last) || last === '刚刚') {
      relTime = last;
      titleLines = lines.slice(0, -1);
    }
    const title = (titleLines[0] || '(无标题)').replace(/\s+/g, ' ').trim().slice(0, 120);

    // 模型：对话项里**没有**模型名（实测：项内只有标题与时间两段）。
    // 唯一可靠的模型来源是**当前对话**顶部的模型选择器，
    // 而 WorkBuddy 的消耗条（「共消耗 9.68 Space-Bunny 16:55」）在
    // 对话区的消息尾部，不在侧栏。所以这里如实留空，由上层标「未显示」。
    const model = '';

    return {
      index: idx,
      convId,
      title,
      relTime,
      model,
      selected,
      visible: r.width > 40 && r.height > 10,
      cls: cls.slice(0, 80),
    };
  });

  // ② 当前对话的模型选择器（唯一可靠的模型来源）
  const modelBtn = document.querySelector('button.cr-model-selector__trigger');
  const currentModel = modelBtn
    ? (modelBtn.getAttribute('title') || txt(modelBtn))
    : '';

  // ③ 当前对话标题（顶栏）
  const topTitle = document.querySelector('span.workbuddy-topbar-title');
  const conversationTitle = topTitle ? txt(topTitle) : '';

  // ③b 当前对话的 conversationId：正在渲染的那条消息所属对话。
  // 侧栏选中项即当前对话，取其 convId 即可。
  const currentConvId = (convs.find((c) => c.selected) || {}).convId || '';

  // ④ 发送键状态：--sending 表示正在生成，此时不能发
  const sendBtn = document.querySelector('button.cr-send-button');
  const sendClass = sendBtn ? (typeof sendBtn.className === 'string' ? sendBtn.className : '') : '';
  const sending = /--sending/.test(sendClass);
  const sendLabel = sendBtn ? (sendBtn.getAttribute('aria-label') || txt(sendBtn) || '') : '';

  // ⑤ 编辑器
  const ed = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  const edChars = ed ? txt(ed).length : -1;
  const edVisible = ed ? (() => {
    const r = ed.getBoundingClientRect();
    return r.width > 40 && r.height > 18;
  })() : false;

  // ⑥ 版本
  const ver = document.querySelector('.conversation-list-version-badge');
  const version = ver ? txt(ver) : '';

  return {
    convs,
    currentModel,
    conversationTitle,
    currentConvId,
    sending,
    sendLabel,
    edChars,
    edVisible,
    version,
  };
};

/**
 * 连 CDP 并采集一份快照。
 *
 * @param {import('playwright-core').Browser} browser 已连上的浏览器
 * @returns {Promise<object>} 快照（纯 JSON）
 */
export async function probe(browser) {
  const ctx = browser.contexts()[0];
  if (!ctx) throw new Error('没有浏览器上下文：CDP 连上了但拿不到 context');
  const pages = ctx.pages().filter((p) => !p.url().startsWith('devtools'));
  const page = pages[0];
  if (!page) throw new Error('找不到渲染页：CDP 通了但没有可用 page');

  const raw = await page.evaluate(PAGE_PROBE);
  const cwds = readSessionCwds();

  // ★ 精确配对 ★
  // 用 data-conversation-id（=== sessionId）直接查表，不做任何猜测性排序。
  // 查不到就如实标「未查到」——留空比错配好：错配的路径看起来是真的。
  const convs = raw.convs.map((c) => {
    const hit = c.convId ? cwds.get(c.convId) : undefined;
    return {
      ...c,
      cwd: hit ? hit.cwd : '',
      cwdConfidence: hit ? '精确（conversation-id 命中）' : (c.convId ? '未查到该 session' : '无 conversation-id'),
    };
  });

  // 当前对话的模型：唯一可靠来源是顶栏选择器。
  // 侧栏对话项里**没有**模型信息（如实标注，不编）。
  if (convs.length) {
    for (const c of convs) {
      c.model = c.selected ? raw.currentModel : '';
      c.modelConfidence = c.selected ? '精确（顶栏选择器）' : '侧栏不显示，需点开该对话才能读';
    }
  }

  return {
    at: Date.now(),
    version: raw.version,
    conversationTitle: raw.conversationTitle,
    currentConvId: raw.currentConvId,
    currentModel: raw.currentModel,
    sending: raw.sending,
    sendLabel: raw.sendLabel,
    editor: { chars: raw.edChars, visible: raw.edVisible },
    convs,
    sessionCount: cwds.size,
    cwdHistogram: [...groupByCwd(cwds).entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 6)
      .map(([cwd, n]) => ({ cwd, sessions: n })),
  };
}
