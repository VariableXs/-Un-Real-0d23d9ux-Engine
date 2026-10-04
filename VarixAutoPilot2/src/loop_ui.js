// ═══════════════════════════════════════════════════════════════════
// 待发队列 / 循环发布 / 内容来源
//
// ★ 边界原则（与主流程一致）★
// 前端**不自己判断"这条发成功了吗"**——那必须由后端用客观证据判定
// （编辑器清空 / 消息入流）。前端只负责展示后端结论。
// ★ 也不自己数轮数★：无限循环在后端是 u32::MAX，
//   前端传 0 表示无限，避免大数字在前端丢精度。
// ═══════════════════════════════════════════════════════════════════

// ★ 共享契约从 window.VAP 取（见 app.js 里的说明）★
const { api, $, esc, renderErr, setScene, conn } = window.VAP;

const Q = {
  picked: '',        // 选中的对话 conv_id（'' = 当前对话）
  pickedTitle: '当前选中的对话',
  loopRunning: false,
  loopTexts: { first: '', rest: '' },
  curTab: 'first',
  skills: [],
};

// ── 对话点选 ────────────────────────────────────────────
function pickConv(convId, title) {
  Q.picked = convId || '';
  Q.pickedTitle = title || '当前选中的对话';
  // 重绘卡片（只改 picked 类，不重建——避免打断悬停）
  for (const el of document.querySelectorAll('.conv')) {
    el.classList.toggle('picked', (el.dataset.cid || '') === Q.picked && !!Q.picked);
  }
  const sel = $('lp-conv');
  if (sel) sel.options[0].textContent = '当前选中：' + Q.pickedTitle.slice(0, 22);
  const n = $('pick-note');
  if (n) n.textContent = Q.picked ? '已选：' + Q.pickedTitle.slice(0, 26) : '未选（发到当前对话）';
}

// ═══ 导出到window ═══
// 模块作用域互不可见，本文件里的函数要��� app.js 与 HTML 的 onclick 用，
// 就必须挂到 window。统一挂一份VAPUI，避免散落多个全局。
// ★ 教训（这轮踩的）：跨模块调用若只挂一半，
//   页面会报 "xxx is not defined"，而 build 与 node --check 全绿 ★
const VAPUI = {
  pickConv, enqueueOne, renderQueue, refreshQueue, clearQueue,
  switchLoopTab, loopTexts, startLoop, stopLoop, readMd, probeSkills, appendSkill,
};
window.VAPUI = VAPUI;
// ★★ 必须同时挂到 window 顶层 ★★
// HTML 里的 onclick="startLoop()" 是在**全局作用域**里找 startLoop，
// 只挂 window.VAPUI.startLoop 它找不到——
// 实测症状：VAPUI 有 12 个键，但点击报 "startLoop is not defined"。
// 这类错 build 与 node --check 都发现不了，只有真点一下才暴露。
Object.assign(window, VAPUI);

// ── 入队一条 ──────────────────────────────────────────────
async function enqueueOne() {
  try {
    const tpl = $('tpl').value;
    const r = await api.preview(tpl, { '任务': $('v-task').value, '上下文': $('v-ctx').value });
    if (r.missing.length) {
      return renderErr({
        what: `还有 ${r.missing.length} 个占位符没填，暂不入队`,
        why: r.missing.map((k) => `{{${k}}}`).join('、'),
        next: '未填的会原样保留，发出去对方会看到一堆 {{}}。填好或删掉再入队',
      });
    }
    const v = await api.enqueue(r.text, Q.picked, 0);
    renderQueue(v.items);
    setHint(`已入队 · ${r.chars} 字符 · 对方空闲时会自动发`, 'ok');
  } catch (e) {
    renderErr(e);
  }
}

// ── 队列渲染 ──────────────────────────────────────────────
const ST_TEXT = { pending: '等待', sending: '发送中', done: '已发', failed: '失败', canceled: '已取消' };

function renderQueue(items) {
  const box = $('qlist');
  box.innerHTML = '';
  if (!items || !items.length) {
    box.innerHTML = '<div class="empty">队列为空。点「加入待发」把内容排进来，或在下面设循环。</div>';
    $('q-sum').textContent = '0 条';
    return;
  }
  for (const it of items) {
    const d = document.createElement('div');
    d.className = 'qi';
    const rd = document.createElement('div');
    rd.className = 'rd';
    rd.textContent = it.round > 0 ? '第' + it.round + '轮' : '单发';
    const pv = document.createElement('div');
    pv.className = 'pv';
    pv.textContent = it.preview;
    pv.title = (it.err ? it.err + ' · ' : '') + it.text;
    const st = document.createElement('div');
    st.className = 'st ' + it.state;
    st.textContent = ST_TEXT[it.state] || it.state;
    const x = document.createElement('button');
    x.className = 'x';
    x.textContent = '×';
    x.title = it.state === 'pending' ? '取消这条' : '这条已不在等待中，不能撤';
    x.disabled = it.state !== 'pending';
    x.onclick = async () => {
      try {
        const v = await api.queueCancel(it.id);
        renderQueue(v.items);
      } catch (e) {
        renderErr(e);
      }
    };
    d.append(rd, pv, st, x);
    box.append(d);
  }
  const wait = items.filter((i) => i.state === 'pending').length;
  const done = items.filter((i) => i.state === 'done').length;
  $('q-sum').textContent = `待发 ${wait} · 已发 ${done} · 共 ${items.length}`;
  $('q-sum').className = 'chip' + (wait ? ' busy' : done ? ' on' : '');
}

async function refreshQueue() {
  try {
    const v = await api.queueView();
    renderQueue(v.items);
  } catch (e) {
    renderErr(e);
  }
}

async function clearQueue() {
  try {
    const v = await api.queueClear();
    renderQueue(v.items);
  } catch (e) {
    renderErr(e);
  }
}

// ── 循环 ─────────────────────────────────────────────────
function switchLoopTab(which) {
  // ★切换标签前先存当前框的内容 ★
  Q.loopTexts[Q.curTab] = $('loop-text').value;
  Q.curTab = which;
  $('loop-text').value = Q.loopTexts[which] || '';
  for (const b of document.querySelectorAll('.tab[data-lt]')) {
    b.classList.toggle('on', b.dataset.lt === which);
  }
}

function loopTexts() {
  Q.loopTexts[Q.curTab] = $('loop-text').value;
  // 首轮为空时退回用后续轮的（常见用法：一套内容循环发）
  const first = (Q.loopTexts.first || '').trim();
  const rest = (Q.loopTexts.rest || '').trim();
  if (first && rest) return [first, rest];
  if (first) return [first, first];
  if (rest) return [rest];
  return [];
}

async function startLoop() {
  const texts = loopTexts();
  if (!texts.length) {
    return setHint('循环没内容：先在上面的框里写点东西', 'err');
  }
  const inf = $('lp-inf').checked;
  const rounds = inf ? 0 : Math.max(1, Math.min(99999, parseInt($('lp-rounds').value, 10) || 1));
  try {
    const v = await api.loopStart(
      texts,
      Q.picked,
      rounds,
      $('lp-trigger').value,
      Math.max(1, parseInt($('lp-interval').value, 10) || 30),
      600
    );
    renderQueue(v.items);
    Q.loopRunning = true;
    $('loop-state').textContent = inf ? '运行中 · 无限' : '运行中 · ' + rounds + ' 轮';
    $('loop-state').className = 'chip on';
    $('loop-note').textContent = inf
      ? '无限循环：会一直发下去，停止请点「停止」'
      : '共 ' + rounds + ' 轮（首轮 + 后续 ' + (rounds - 1) + ' 轮）';
    setHint(inf ? '无限循环已开始' : '循环已开始，共 ' + rounds + ' 轮', 'ok');
  } catch (e) {
    renderErr(e);
  }
}

async function stopLoop() {
  try {
    const v = await api.loopStop();
    renderQueue(v.items);
    Q.loopRunning = false;
    $('loop-state').textContent = '已停止';
    $('loop-state').className = 'chip';
    $('loop-note').textContent = '队列里未发的项仍保留，可继续或清空';
    setHint('循环已停。未发的项还在队列里', 'ok');
  } catch (e) {
    renderErr(e);
  }
}

// ── 内容来源：MD 文件 ─────────────────────────────────────
async function readMd() {
  const path = $('md-path').value.trim();
  if (!path) return setHint('先填文件路径，或把文件拖到路径框里', 'err');
  try {
    const f = await api.readTextFile(path);
    $('md-info').innerHTML =
      `<span class="e-next">${esc(f.name)} · ${f.chars} 字符 / ${f.bytes} 字节 · 编码 ${esc(f.encoding)}</span>`;
    // 填进「首轮」与「后续轮」——但只在它们为空时填，不覆盖用户已写的
    if (!Q.loopTexts.first.trim()) { Q.loopTexts.first = f.text; Q.loopTexts.rest = f.text; }
    else if (!Q.loopTexts.rest.trim()) { Q.loopTexts.rest = f.text; }
    $('loop-text').value = Q.loopTexts[Q.curTab];
    setHint(`已读入 ${f.name}（${f.chars} 字符）。若下面框里已有内容，我没覆盖它`, 'ok');
  } catch (e) {
    renderErr(e);
  }
}

// ── 内容来源：技能（斜杠命令） ───────────────────────────
async function probeSkills() {
  const info = $('skill-info');
  info.innerHTML = '<span class="e-what">正在探测…（会往输入框打一个斜杠再清掉）</span>';
  try {
    const r = await api.probeSkills();
    Q.skills = r.items || [];
    const sel = $('skill-pick');
    sel.innerHTML = '<option value="">— 选择技能 —</option>';
    for (const s of Q.skills) {
      const o = document.createElement('option');
      o.value = s;
      o.textContent = s;
      sel.append(o);
    }
    if (!r.cleaned) {
      // ★ 显式提示残留，不静默 ★
      info.innerHTML =
        '<span class="e-what">探测后输入框有残留</span>' +
        '<span class="e-why">清理动作没成功，那一个斜杠可能还在</span>' +
        '<span class="e-next">切到 WorkBuddy 窗口手动删掉那个 /，再重新探测</span>';
      setHint('探测完成，但清理不干净——见技能区提示', 'err');
    } else {
      info.innerHTML = Q.skills.length
        ? `<span class="e-next">找到 ${Q.skills.length} 个可用命令（读自你的 WorkBuddy，不是预置列表）</span>`
        : '<span class="e-what">没抓到技能清单</span>' +
          '<span class="e-why">WorkBuddy 的补全面板可能换了结构</span>' +
          '<span class="e-next">可以直接在内容框里手打 /命令名 试试</span>';
      setHint(Q.skills.length ? `探测到 ${Q.skills.length} 个可用命令` : '没抓到技能清单', Q.skills.length ? 'ok' : 'err');
    }
  } catch (e) {
    info.innerHTML = '';
    renderErr(e);
  }
}

function appendSkill() {
  const s = $('skill-pick').value;
  if (!s) return setHint('先选一个技能', 'err');
  //技能名是「/命令名 描述」形态，插入时只取命令部分
  const cmd = s.trim().split(/\s+/)[0];
  Q.loopTexts[Q.curTab] = $('loop-text').value;
  Q.loopTexts[Q.curTab] += (Q.loopTexts[Q.curTab] ? '\n' : '') + cmd + ' ';
  $('loop-text').value = Q.loopTexts[Q.curTab];
  setHint('已插入 ' + cmd + ' 到「' + (Q.curTab === 'first' ? '首轮' : '后续每轮') + '」', 'ok');
}

// ── 统一提示（与主流程同一套三要素样式）─────────────────
function setHint(text, kind) {
  const h = $('hint');
  h.className = 'note' + (kind === 'err' ? '' : kind === 'ok' ? ' ok' : '');
  h.textContent = text;
}
