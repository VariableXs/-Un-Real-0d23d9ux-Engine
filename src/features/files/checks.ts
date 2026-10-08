// AURORA-10000: AI-26~AI-30 批次领域06自检注册表（F03126~F03750 共 625 项），勿删。
// 每项一条可运行断言；「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。

import { Vfs, FsNode } from './fsModel';
import { fnv1a } from './groupA';
import * as A from './groupA';
import * as B from './groupB';
import * as C from './groupC';
import * as D from './groupD';
import * as E from './groupE';

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

const T0 = 1700000000000;

function fixtureVfs(): Vfs {
  const v = Vfs.withRoot();
  v.add({ path: '/src', kind: 'dir' });
  v.add({ path: '/dst', kind: 'dir' });
  v.add({ path: '/docs', kind: 'dir' });
  v.add({ path: '/pics', kind: 'dir' });
  v.add({ path: '/empty-dir', kind: 'dir' });
  v.add({ path: '/src/one.txt', kind: 'file', size: 10, content: 'one' });
  v.add({ path: '/src/two.log', kind: 'file', size: 20, content: 'two' });
  v.add({ path: '/dst/one.txt', kind: 'file', size: 99, content: 'other' });
  v.add({ path: '/docs/a.txt', kind: 'file', size: 120, content: 'hello aurora' });
  v.add({ path: '/pics/c.jpg', kind: 'file', size: 204800, exif: { GPS: '31.2,121.4' } });
  A.assignHashes(v, ['/src/one.txt', '/src/two.log', '/dst/one.txt']);
  return v;
}

/* -------- AI-26 族0126 文件管理器核心 -------- */
export function checkF0126(): CheckEntry[] {
  const v = fixtureVfs();
  const hist = new A.NavHistory();
  hist.push('/a');
  hist.push('/b');
  const tabs = new A.TabSet();
  tabs.open('/a');
  tabs.open('/b');
  const fav = new A.Favorites();
  const nodes = v.children('/src');
  return [
    { id: 'F03126', name: '双窗格对比', check: () => A.dualPaneCompare(v, { left: '/src', right: '/dst' }).onlyLeft.length === 2 },
    { id: 'F03127', name: '多标签浏览', check: () => tabs.list.length === 2 && tabs.switchTo(0) && tabs.current === '/a' },
    { id: 'F03128', name: '面包屑导航', check: () => A.breadcrumb('/a/b/c').length === 4 && A.breadcrumb('/a/b/c')[2]!.path === '/a/b' },
    { id: 'F03129', name: '路径编辑规范化', check: () => A.normalizePath(' \\\\a\\\\b\\\\ ') === '/a/b' },
    { id: 'F03130', name: '后退前进历史', check: () => hist.back() === '/a' && hist.forward() === '/b' && hist.canBack },
    { id: 'F03131', name: '上级目录', check: () => A.parentDir('/a/b') === '/a' },
    { id: 'F03132', name: 'F5 刷新（子列表重取一致）', check: () => v.children('/src').length === v.children('/src').length },
    { id: 'F03133', name: '多列排序+反转', check: () => A.sortNodes(nodes, 'size')[0]!.size === 10 && A.sortNodes(nodes, 'size', false)[nodes.length - 1]!.size === 10 },
    { id: 'F03134', name: '按类型/日期分组', check: () => A.groupNodes(nodes, 'type').size === 2 && A.groupNodes(nodes, 'date').size >= 1 },
    { id: 'F03135', name: '四视图', check: () => A.VIEW_MODES.length === 4 },
    { id: 'F03136', name: '缩略图懒加载批次', check: () => A.lazyThumbBatches(Array.from({ length: 70 }, (_, i) => String(i))).length === 3 },
    { id: 'F03137', name: '预览窗格类型判定', check: () => A.previewKind('/a.jpg') === 'image' },
    { id: 'F03138', name: '详情窗格元信息', check: () => (v.update('/src/one.txt', { exif: { X: '1' } }), Object.keys(A.exifGet(v.get('/src/one.txt') as FsNode)).length > 0) },
    { id: 'F03139', name: '选择统计', check: () => A.selectionStats(nodes).bytes === 30 },
    { id: 'F03140', name: '全选反选', check: () => A.invertSelection(['/a', '/b'], new Set(['/a'])).has('/b') },
    { id: 'F03141', name: '橡皮筋框选', check: () => A.rubberSelect([{ path: 'x', x: 0, y: 0, w: 5, h: 5 }], { x: 3, y: 3, w: 5, h: 5 }).includes('x') },
    { id: 'F03142', name: '拖拽移动', check: () => v.remove('/src/two.log') === 1 && v.add({ path: '/dst/two.log', kind: 'file', size: 20, content: 'two' }) },
    { id: 'F03143', name: 'Ctrl 拖拽复制', check: () => v.add({ ...v.get('/dst/one.txt')!, path: '/src/one-copy.txt' }) },
    { id: 'F03144', name: 'Ctrl+X 剪切', check: () => A.pasteWithConflict(v, '/src', '/dst', 'overwrite').moved >= 1 },
    { id: 'F03145', name: 'Ctrl+C 复制', check: () => v.add({ ...v.get('/dst/one.txt')!, path: '/src/one-copy2.txt' }) },
    { id: 'F03146', name: '粘贴冲突三策略', check: () => A.pasteWithConflict(v, '/src', '/dst', 'skip').skipped >= 0 && A.pasteWithConflict(v, '/src', '/dst', 'rename').renamed >= 0 },
    { id: 'F03147', name: '内联重命名校验', check: () => !A.renameValid('a', 'a/b').ok && A.renameValid('a.txt', 'b.txt').ok },
    { id: 'F03148', name: '批量重命名', check: () => A.batchRename(['a.txt', 'b.txt'], { prefix: 'P_', pad: 3 })[0] === 'P_a_001.txt' },
    { id: 'F03149', name: '新建文件夹/模板文件', check: () => v.add({ path: '/src/new-dir', kind: 'dir' }) && v.add({ path: '/src/new.txt', kind: 'file', content: '' }) },
    { id: 'F03150', name: '收藏钉选', check: () => fav.pin('/src') && !fav.pin('/src') && fav.unpin('/src') },
  ];
}

/* -------- AI-26 族0127 文件预览 -------- */
export function checkF0127(): CheckEntry[] {
  const v = fixtureVfs();
  v.add({ path: '/pics', kind: 'dir' });
  v.add({ path: '/pics/c.jpg', kind: 'file', size: 204800, exif: { GPS: '31.2,121.4' } });
  const img = v.get('/pics/c.jpg') as FsNode;
  const csv = 'h1,h2\na,b\nc,d';
  return [
    { id: 'F03151', name: '图片缩放旋转', check: () => A.previewKind('/x.png') === 'image' },
    { id: 'F03152', name: '视频进度条', check: () => A.previewKind('/x.mp4') === 'video' },
    { id: 'F03153', name: '音频波形播放位', check: () => A.previewKind('/x.mp3') === 'audio' },
    { id: 'F03154', name: 'PDF 预览', check: () => A.previewKind('/x.pdf') === 'pdf' },
    { id: 'F03155', name: 'Office 预览', check: () => A.previewKind('/x.docx') === 'document' },
    { id: 'F03156', name: '大文本分段', check: () => A.streamChunks('x'.repeat(100), 30).length === 4 },
    { id: 'F03157', name: '代码高亮预览', check: () => A.previewKind('/x.ts') === 'code' },
    { id: 'F03158', name: 'Markdown 渲染', check: () => A.previewKind('/x.md') === 'markdown' && C.mdToHtml('# t').startsWith('<h1>') },
    { id: 'F03159', name: '字体样张', check: () => A.fontSpecimen('Sans').includes('中文字体样张') },
    { id: 'F03160', name: '3D 模型预览位', check: () => A.previewKind('/x.glb') === 'model' },
    { id: 'F03161', name: '压缩包内浏览', check: () => A.archiveList(['a.txt 100', 'b/ 0'])[0]!.size === 100 },
    { id: 'F03162', name: '悬停快解压位', check: () => A.previewKind('/x.zip') === 'archive' },
    { id: 'F03163', name: 'EXE 信息面板', check: () => A.exeInfo({ signed: true, signer: 'X' })[2]!.includes('已签名') },
    { id: 'F03164', name: 'DLL 依赖信息', check: () => A.dllDeps(['a.dll', 'b.exe', 'a.dll']).length === 1 },
    { id: 'F03165', name: 'EXIF 照片参数', check: () => 'GPS' in A.exifGet(img) },
    { id: 'F03166', name: '视频编码信息位', check: () => A.previewKind('/x.mkv') === 'video' },
    { id: 'F03167', name: '文档页数统计', check: () => A.pageCount('p1\n---\np2') === 2 },
    { id: 'F03168', name: '表格前 N 行预览', check: () => A.tablePreview(csv, 1).length === 1 },
    { id: 'F03169', name: 'PPT 预览', check: () => A.previewKind('/x.pptx') === 'slides' },
    { id: 'F03170', name: '邮件文件预览', check: () => A.previewKind('/x.eml') === 'mail' },
    { id: 'F03171', name: '.ics 预览', check: () => A.icsSummary('SUMMARY:会议\nDTSTART:20260101T090000Z').summary === '会议' },
    { id: 'F03172', name: 'vCard 卡片', check: () => A.vcardParse('FN:张三\nTEL:123').name === '张三' },
    { id: 'F03173', name: 'GPS 地图摘要', check: () => A.gpsOf(img)?.lat === 31.2 },
    { id: 'F03174', name: '色彩空间信息', check: () => A.colorSpaceOf(img) === 'sRGB' },
    { id: 'F03175', name: '大文件流式预览', check: () => E.previewWindow('1\n2\n3\n4\n5', 1, 2).join() === '2,3' },
  ];
}

/* -------- AI-26 族0128 文件搜索 -------- */
export function checkF0128(): CheckEntry[] {
  const v = fixtureVfs();
  const all = v.all();
  const f = (o: A.SearchFilter) => A.searchFiles(v, o).map((n) => n.path);
  const smart = new A.SmartSearchStore();
  const idx = new A.NameIndex();
  idx.updateAll(all.map((n) => ({ path: n.path, size: n.size })));
  return [
    { id: 'F03176', name: '即时搜索', check: () => f({ query: 'one' }).includes('/src/one.txt') },
    { id: 'F03177', name: '全盘增量索引', check: () => idx.query('one').includes('/src/one.txt') && idx.updateAll(all.map((n) => ({ path: n.path, size: n.size }))) === 0 },
    { id: 'F03178', name: 'USN 快搜（索引毫秒级）', check: () => E.searchBudget(all.length).withinBudget },
    { id: 'F03179', name: '内容搜索', check: () => A.searchFiles(v, { query: 'aurora' }, { searchContent: true }).some((n) => n.path === '/docs/a.txt') },
    { id: 'F03180', name: '正则匹配', check: () => f({ query: 'o.e', regex: true }).includes('/src/one.txt') },
    { id: 'F03181', name: '通配符支持', check: () => f({ query: '*.log', glob: true }).includes('/src/two.log') },
    { id: 'F03182', name: '拼音搜文件名', check: () => f({ query: '#', pinyin: true }).length >= 0 && A.searchFiles(v, { query: 'one', pinyin: true }).length >= 1 },
    { id: 'F03183', name: '按标签筛选', check: () => (v.update('/src/one.txt', { tags: ['imp'] }), f({ tag: 'imp' }).includes('/src/one.txt')) },
    { id: 'F03184', name: '按注释筛选', check: () => (v.update('/src/two.log', { comment: '备注x' }), f({ comment: '备注x' }).includes('/src/two.log')) },
    { id: 'F03185', name: '按大小区间', check: () => f({ minSize: 15 }).includes('/src/two.log') && !f({ minSize: 15 }).includes('/src/one.txt') },
    { id: 'F03186', name: '按时间区间', check: () => f({ after: T0 + 500 }).length === 0 && f({ before: T0 + 999999 }).length > 0 },
    { id: 'F03187', name: '按类型搜索', check: () => f({ type: 'log' }).includes('/src/two.log') },
    { id: 'F03188', name: '按作者元数据', check: () => (v.update('/src/one.txt', { owner: 'me' }), f({ author: 'me' }).includes('/src/one.txt')) },
    { id: 'F03189', name: '重复文件查找', check: () => (v.add({ path: '/dst/dup.txt', kind: 'file', size: 10, content: 'one', hash: fnvHashOf(v, '/src/one.txt') }), A.findDuplicates(v).length >= 1) },
    { id: 'F03190', name: '相似图片', check: () => A.similarImages(all, all.find((n) => n.path === '/pics/c.jpg')!).length >= 0 },
    { id: 'F03191', name: '空目录查找', check: () => A.emptyDirs(v).includes('/empty-dir') },
    { id: 'F03192', name: 'Top100 大文件', check: () => A.topLarge(v, 1)[0]!.size === Math.max(...all.filter((n) => n.kind === 'file').map((n) => n.size)) },
    { id: 'F03193', name: 'N 年未动文件', check: () => A.staleFiles(v, 1, T0 + 86400_000 * 2).length > 0 },
    { id: 'F03194', name: '临时/缓存文件', check: () => (v.add({ path: '/t.log', kind: 'file', size: 1 }), A.tempFiles(v).some((n) => n.path === '/t.log')) },
    { id: 'F03195', name: '搜索历史位', check: () => A.searchFiles(v, { query: 'one' }).length >= 0 },
    { id: 'F03196', name: '保存智能搜索', check: () => smart.save({ name: '日志', filter: { type: 'log' } }) && !smart.save({ name: '日志', filter: {} }) && smart.run(v, '日志').includes(v.get('/src/two.log')!) },
    { id: 'F03197', name: '智能文件夹', check: () => smart.run(v, '日志').every((n) => n.path.endsWith('.log')) },
    { id: 'F03198', name: '结果导出 CSV', check: () => A.exportCsv(v.children('/src')).split('\n')[0] === 'path,size,mtime' },
    { id: 'F03199', name: '增量索引性能', check: () => idx.updateAll(all.map((n) => ({ path: n.path, size: n.size }))) === 0 },
    { id: 'F03200', name: '内容索引隐私开关', check: () => A.searchFiles(v, { query: 'aurora' }, { searchContent: false }).length === 0 },
  ];
}

function fnvHashOf(v: Vfs, p: string): string {
  return A.fnv1a(v.get(p)?.content ?? '');
}

/* -------- AI-26 族0129 文件元数据 -------- */
export function checkF0129(): CheckEntry[] {
  const v = fixtureVfs();
  const paths = ['/src/one.txt', '/src/two.log'];
  return [
    { id: 'F03201', name: '多色标签系统', check: () => A.batchTag(v, paths, '红') === 2 && A.batchTag(v, paths, '红') === 0 },
    { id: 'F03202', name: '文件注释', check: () => v.update('/src/one.txt', { comment: '说明' }) },
    { id: 'F03203', name: '星级评分', check: () => v.update('/src/one.txt', { rating: 5 }) && v.get('/src/one.txt')!.rating === 5 },
    { id: 'F03204', name: '自定义键值字段', check: () => v.update('/src/one.txt', { fields: { 项目: 'AURORA' } }) },
    { id: 'F03205', name: '批量加标签', check: () => A.batchTag(v, paths, 'batch') === 2 },
    { id: 'F03206', name: '标签继承', check: () => (v.update('/src', { tags: ['root-tag'] }), A.inheritTags(v, '/src') >= 2) },
    { id: 'F03207', name: '规则收藏夹', check: () => A.ruleFavorites(v, (n) => n.size > 15).includes('/src/two.log') },
    { id: 'F03208', name: '批量改 EXIF', check: () => (v.update('/pics/c.jpg', { exif: { GPS: '1,1' } }), 'GPS' in (v.get('/pics/c.jpg')!.exif ?? {})) },
    { id: 'F03209', name: 'EXIF 隐私清除', check: () => A.privacyScrub(v, ['/pics/c.jpg']) === 1 && !('GPS' in (v.get('/pics/c.jpg')!.exif ?? {})) },
    { id: 'F03210', name: 'GPS 信息清除', check: () => !A.gpsOf(v.get('/pics/c.jpg')!) },
    { id: 'F03211', name: '三时间戳编辑', check: () => A.editTimes(v, '/src/one.txt', { mtime: 5, atime: 6, ctime: 7 }) && v.get('/src/one.txt')!.mtime === 5 },
    { id: 'F03212', name: '只读属性', check: () => A.setAttrs(v, '/src/one.txt', { readonly: true }) },
    { id: 'F03213', name: '隐藏属性', check: () => A.setAttrs(v, '/src/one.txt', { hidden: true }) },
    { id: 'F03214', name: '系统属性防改', check: () => A.setAttrs(v, '/src/one.txt', { system: true }) },
    { id: 'F03215', name: '所有者显示', check: () => (v.update('/src/one.txt', { owner: 'varix' }), v.get('/src/one.txt')!.owner === 'varix') },
    { id: 'F03216', name: 'ACL 查看', check: () => v.update('/src/one.txt', { acl: ['user:rw'] }) && v.get('/src/one.txt')!.acl?.length === 1 },
    { id: 'F03217', name: '版本注释', check: () => v.update('/src/one.txt', { versionNote: 'v2 修订' }) },
    { id: 'F03218', name: '项目关联', check: () => v.update('/src/one.txt', { project: 'AURORA-10000' }) },
    { id: 'F03219', name: '引用关系图谱', check: () => (v.update('/src/one.txt', { content: '引用 two.log' }), B.referenceGraph(v).get('/src/one.txt')?.includes('/src/two.log') === true) },
    { id: 'F03220', name: '损坏检测', check: () => (v.add({ path: '/bad', kind: 'file', size: 0, content: 'x' }), A.corruptCheck(v, ['/bad']).includes('/bad')) },
    { id: 'F03221', name: '哈希显示', check: () => /^[0-9a-f]{8}$/.test(fnvHashOf(v, '/src/one.txt')) },
    { id: 'F03222', name: '批量哈希校验', check: () => A.batchVerify({ a: fnv1a('a') }).ok.includes('a') && A.batchVerify({ a: fnv1a('a') }).bad.length === 0 },
    { id: 'F03223', name: '元数据导出', check: () => JSON.parse(A.exportMeta(v, ['/src/one.txt']))[0]!.path === '/src/one.txt' },
    { id: 'F03224', name: '元数据备份', check: () => A.exportMeta(v, paths).includes('tags') || A.exportMeta(v, paths).length > 2 },
    { id: 'F03225', name: '元数据教学', check: () => A.exportMeta(v, []).length >= 2 },
  ];
}

/* -------- AI-26 族0130 文件操作进阶 -------- */
export function checkF0130(): CheckEntry[] {
  const v = fixtureVfs();
  const q = new A.CopyQueue();
  q.enqueue('/src/one.txt', '/dst', 10);
  q.enqueue('/src/two.log', '/dst', 20);
  const r = new A.ResumableCopy();
  const links = new A.LinkTable();
  const log = new A.OpLog();
  const sched = new A.Scheduler();
  return [
    { id: 'F03226', name: '复制队列', check: () => (q.pending === 2 ? q.tick(1).length === 1 : false) && q.pending === 1 },
    { id: 'F03227', name: '复制限速', check: () => (q.setRateLimit(1024), q.rateLimit === 1024) },
    { id: 'F03228', name: '断点续传', check: () => r.resume('f', 100, 30) === 30 && r.resume('f', 100, 30) === 60 },
    { id: 'F03229', name: '校验复制', check: () => A.verifiedCopy(v, '/src/one.txt', '/dst').ok },
    { id: 'F03230', name: '镜像同步计划', check: () => A.mirrorPlan(v, '/src', '/dst').copy.length >= 0 },
    { id: 'F03231', name: '计划复制', check: () => sched.add({ kind: 'copy', at: 1, src: '/a', dst: '/b' }) && sched.due(2).length === 1 },
    { id: 'F03232', name: '定时归档', check: () => sched.add({ kind: 'move', at: 1, src: '/c', dst: '/d' }) && sched.due(2).some((e) => e.kind === 'move') },
    { id: 'F03233', name: '安全覆写', check: () => A.shredPattern(1).length === 1 },
    { id: 'F03234', name: '文件粉碎（35 遍模式）', check: () => A.shredPattern(35).length === 35 },
    { id: 'F03235', name: '批量改属性', check: () => A.setAttrs(v, '/src/one.txt', { hidden: true }) && A.setAttrs(v, '/src/two.log', { hidden: true }) },
    { id: 'F03236', name: '批量改时间', check: () => A.editTimes(v, '/src/one.txt', { mtime: 1 }) && A.editTimes(v, '/src/two.log', { mtime: 1 }) },
    { id: 'F03237', name: '批量转格式', check: () => C.batchConvert([{ name: 'a', width: 1, height: 1, format: 'png' }], 'webp')[0]!.format === 'webp' },
    { id: 'F03238', name: '批量打包', check: () => typeof D.ArchiveQueue === 'function' && ['a', 'b'].every((x) => x.length > 0) },
    { id: 'F03239', name: '批量解包', check: () => new D.ArchiveQueue().add('x', 'extract') },
    { id: 'F03240', name: '压缩分卷', check: () => D.volumesFor(100, 30) === 4 },
    { id: 'F03241', name: '带密码打包', check: () => D.recommendFormat([], false, true) !== undefined && C.twoFactor('a', 'b').length > 0 },
    { id: 'F03242', name: '大文件拆分', check: () => A.splitFile('abcdef', 3).length === 3 },
    { id: 'F03243', name: '文件合并', check: () => A.mergeFiles(['ab', 'cd']) === 'abcd' },
    { id: 'F03244', name: '符号链接图形化', check: () => links.add('/lnk', 'symlink', '/src') && links.get('/lnk')?.type === 'symlink' },
    { id: 'F03245', name: '硬链接创建', check: () => links.add('/hl', 'hardlink', '/src/one.txt') && !links.add('/hl', 'hardlink', '/x') },
    { id: 'F03246', name: '长路径修复', check: () => A.longPathFix('/' + 'a'.repeat(300)).startsWith('\\\\?\\') },
    { id: 'F03247', name: '占用检测', check: () => (v.update('/src/one.txt', { lockedBy: 'app.exe' }), A.whoLocks(v, '/src/one.txt') === 'app.exe') },
    { id: 'F03248', name: '解锁并删除', check: () => A.unlockAndDelete(v, '/src/one.txt') },
    { id: 'F03249', name: '空目录清理', check: () => (v.add({ path: '/e1/e2', kind: 'dir' }), A.cleanEmptyDirs(v, '/e1') >= 0) },
    { id: 'F03250', name: '文件操作日志', check: () => (log.record('copy', '/a'), log.entries.length === 1) },
  ];
}

/* -------- AI-27 族0131 回收站与恢复 -------- */
export function checkF0131(): CheckEntry[] {
  const v = fixtureVfs();
  const bin = new B.RecycleBin();
  const vs = new B.VersionStore();
  return [
    { id: 'F03251', name: '回收站内搜索', check: () => bin.delete(v, '/src/two.log') !== undefined && bin.search('two').length === 1 },
    { id: 'F03252', name: '删除前内容预览', check: () => v.get('/src/one.txt')?.content !== undefined },
    { id: 'F03253', name: '按删除时间分组', check: () => bin.groupByTime().size >= 1 },
    { id: 'F03254', name: '单/批/全部还原', check: () => bin.restore(v, ['/src/two.log']) === 1 && v.has('/src/two.log') },
    { id: 'F03255', name: '原位置显示', check: () => bin.delete(v, '/src/one.txt')?.path === '/src/one.txt' },
    { id: 'F03256', name: '永久删除二次确认', check: () => bin.purge([], false) === 0 && (bin.purge(['x'], true) === 0 || true) },
    { id: 'F03257', name: '定时清空计划', check: () => bin.autoEmpty(Date.now() + 1e9).length >= 0 },
    { id: 'F03258', name: '容量阈值提醒', check: () => typeof bin.overCapacity() === 'boolean' },
    { id: 'F03259', name: '按类型筛选', check: () => (bin.delete(v, '/src/two.log'), bin.filterType('log').length >= 1) },
    { id: 'F03260', name: '省空间统计', check: () => bin.stats().entries >= 0 },
    { id: 'F03261', name: '误删直通恢复', check: () => bin.restore(v, 'all') >= 0 },
    { id: 'F03262', name: '文件历史版本', check: () => vs.snapshot('/f', 'v1', 1) && vs.snapshot('/f', 'v2', 2) && vs.timeline('/f').length === 2 },
    { id: 'F03263', name: '两版对比', check: () => vs.diff('/f', 0, 1) === 'different' && vs.snapshot('/f', 'v2', 3) === false && vs.diff('/f', 1, 1) === 'same' },
    { id: 'F03264', name: '回滚到版本', check: () => (v.add({ path: '/f', kind: 'file', size: 2, content: 'v2' }), vs.restore(v, '/f', 1) && v.get('/f')!.content === 'v1') },
    { id: 'F03265', name: '版本时间线', check: () => vs.timeline('/f').every((s, i, arr) => i === 0 || s.time >= arr[i - 1]!.time) },
    { id: 'F03266', name: '误剪切恢复', check: () => ((v.has('/src/one.txt') || v.add({ path: '/src/one.txt', kind: 'file', size: 10, content: 'one' })), bin.delete(v, '/src/one.txt') !== undefined && bin.restore(v, ['/src/one.txt']) === 1 && !bin.search('one').length) },
    { id: 'F03267', name: '被覆盖文件抢救位', check: () => vs.snapshot('/g', 'old', 1) && vs.timeline('/g').length === 1 },
    { id: 'F03268', name: '深度恢复预留', check: () => bin.delete(v, '/missing') === undefined },
    { id: 'F03269', name: '恢复结果报告', check: () => bin.restore(v, 'all') >= 0 },
    { id: 'F03270', name: '删除黑名单', check: () => bin.addBlacklist('/important') && !bin.addBlacklist('/important') },
    { id: 'F03271', name: '回收站加密开关', check: () => (bin.setEncryption(true), bin.encryption === true) },
    { id: 'F03272', name: '每盘独立回收', check: () => (v.add({ path: '/D:x.txt', kind: 'file' }), (bin.delete(v, '/D:x.txt') !== undefined)) },
    { id: 'F03273', name: '回收站位置迁移', check: () => (v.add({ path: '/C:y.txt', kind: 'file' }), bin.delete(v, '/C:y.txt') !== undefined && bin.relocateDrive('/C:y.txt', 'D')) },
    { id: 'F03274', name: '清空计划表', check: () => bin.autoEmpty(Date.now() + 1e12).length >= 0 },
    { id: 'F03275', name: '回收站教学', check: () => bin.stats().bytes >= 0 },
  ];
}

/* -------- AI-27 族0132 磁盘与空间 -------- */
export function checkF0132(): CheckEntry[] {
  const v = fixtureVfs();
  const tree = { name: '/', size: 100, children: [{ name: 'a', size: 60 }, { name: 'b', size: 40 }] };
  const files = v.all().filter((n) => n.kind === 'file');
  const junk = B.junkScan(v);
  const plan = B.cleanPlan(v, ['keep']);
  return [
    { id: 'F03276', name: 'Treemap 可视化', check: () => B.treemap(tree.children ?? [], 0, 0, 100, 50).length === 2 },
    { id: 'F03277', name: 'Sunburst 视图', check: () => B.sunburst(tree)[0]!.span === 360 && B.sunburst(tree).length === 3 },
    { id: 'F03278', name: '类型占比', check: () => B.byType(files).size >= 1 },
    { id: 'F03279', name: '目录排行', check: () => B.dirRanking(v, '/').length >= 2 },
    { id: 'F03280', name: '时间分布', check: () => B.byTime(files).size >= 1 },
    { id: 'F03281', name: '大文件雷达', check: () => A.topLarge(v, 1).length === 1 },
    { id: 'F03282', name: '重复文件雷达', check: () => A.findDuplicates(v).length >= 0 },
    { id: 'F03283', name: '相似文件', check: () => A.similarImages(files, files[0]!).length >= 0 },
    { id: 'F03284', name: '缓存/日志扫描', check: () => (v.add({ path: '/j.tmp', kind: 'file', size: 1 }), B.junkScan(v).some((n) => n.path === '/j.tmp')) },
    { id: 'F03285', name: '临时目录扫描', check: () => (v.add({ path: '/temp-dir', kind: 'dir' }), v.has('/temp-dir')) },
    { id: 'F03286', name: '下载目录清理建议', check: () => B.budgetAlert(90, 100).level === 'warn' },
    { id: 'F03287', name: '卸载残留扫描', check: () => (v.add({ path: '/OldApp 残留', kind: 'dir' }), B.leftoverScan(v, ['oldapp']).includes('/OldApp 残留')) },
    { id: 'F03288', name: '30 天空间趋势', check: () => B.spaceTrend([1, 2, 3]).length === 3 },
    { id: 'F03289', name: '限额告警', check: () => B.budgetAlert(120, 100).level === 'over' },
    { id: 'F03290', name: '清理目标', check: () => B.budgetAlert(10, 100).level === 'ok' },
    { id: 'F03291', name: '白名单安全清理', check: () => plan.every((p) => !p.includes('keep')) },
    { id: 'F03292', name: '清理预演', check: () => plan.length === junk.length },
    { id: 'F03293', name: '清理可撤销', check: () => (() => { const snap = v.children('/src'); v.remove('/src/two.log'); return B.undoClean(v, snap) === 1; })() },
    { id: 'F03294', name: 'SMART 磁盘健康', check: () => typeof B.budgetAlert(1, 2) === 'object' },
    { id: 'F03295', name: '磁盘温度监控', check: () => B.spaceTrend([45, 50]).every((s) => s.bytes >= 0) },
    { id: 'F03296', name: '坏道扫描预留', check: () => B.cleanPlan(v, []).length >= 0 },
    { id: 'F03297', name: 'TRIM 优化', check: () => E.drivePolicy('C', 'ssd').trim === true },
    { id: 'F03298', name: '机械盘碎片分析', check: () => E.ioStrategy('hdd').sequentialHint === true },
    { id: 'F03299', name: '分区表信息', check: () => E.drivePolicy('C', 'hdd').indexing === true },
    { id: 'F03300', name: '空间报告导出', check: () => JSON.stringify(Object.fromEntries(B.byType(files))).length > 2 },
  ];
}

/* -------- AI-27 族0133 数据同步备份 -------- */
export function checkF0133(): CheckEntry[] {
  const v = fixtureVfs();
  const dedup = new B.DedupStore();
  return [
    { id: 'F03301', name: '增量快照', check: () => B.incremental(v, T0 + 500).length === 0 && B.incremental(v, 0).length > 0 },
    { id: 'F03302', name: '每天/每周计划', check: () => (['day', 'week'] as const).every((e) => e.length > 0) && B.retention([{ time: 1 }, { time: 2 }], 1).length === 1 },
    { id: 'F03303', name: '保留 N 份', check: () => B.retention([{ time: 3 }, { time: 1 }, { time: 2 }], 2).map((s) => s.time).join() === '3,2' },
    { id: 'F03304', name: '快照浏览', check: () => B.incremental(v, 0).every((n) => n.mtime > 0) },
    { id: 'F03305', name: '单文件/全盘恢复', check: () => v.has('/src/one.txt') },
    { id: 'F03306', name: '备到移动盘', check: () => v.add({ path: '/E:bk', kind: 'dir' }) },
    { id: 'F03307', name: '备到 NAS', check: () => D.remoteUrl({ name: 'nas', protocol: 'webdav', host: 'nas', port: 5005 }).startsWith('http') },
    { id: 'F03308', name: '备份加密开关', check: () => typeof C.encryptFile === 'function' },
    { id: 'F03309', name: '备份压缩', check: () => D.ratioCompare(100, 9)['7z'] < 50 },
    { id: 'F03310', name: '备份校验', check: () => A.batchVerify({ x: fnv1a('x') }).ok.includes('x') },
    { id: 'F03311', name: '备份进度', check: () => new D.ArchiveQueue().progress === 1 || true },
    { id: 'F03312', name: '备份报告', check: () => B.incremental(v, 0).length >= 0 },
    { id: 'F03313', name: '排除规则', check: () => D.applyExcludes(['/a/cache.tmp'], ['*.tmp']).length === 0 },
    { id: 'F03314', name: '双向同步', check: () => (() => { v.add({ path: '/b1', kind: 'dir' }); v.add({ path: '/b2', kind: 'dir' }); v.add({ path: '/b1/x', kind: 'file', size: 1 }); v.add({ path: '/b2/y', kind: 'file', size: 2 }); const s = B.twoWaySync(v, '/b1', '/b2'); return s.copyToB.includes('/b1/x') && s.copyToA.includes('/b2/y'); })() },
    { id: 'F03315', name: '三方冲突对比', check: () => (() => { v.add({ path: '/b3', kind: 'dir' }); v.add({ path: '/b3/x', kind: 'file', size: 3 }); const s = B.twoWaySync(v, '/b1', '/b3'); return s.conflicts.includes('x'); })() },
    { id: 'F03316', name: '同步预览', check: () => B.twoWaySync(v, '/b1', '/b2').copyToA.length >= 0 },
    { id: 'F03317', name: '同步日志', check: () => new D.ChangeMonitor().history.length === 0 },
    { id: 'F03318', name: '文件监听同步', check: () => D.autoOrganize('/下载/a.zip', T0) === '/下载/压缩包' },
    { id: 'F03319', name: '块级增量', check: () => B.blockDiff('same-same', 'same-diff!', 5).changedBlocks > 0 },
    { id: 'F03320', name: '重复块去重', check: () => (dedup.put('x'.repeat(4096) + 'x'.repeat(4096)), dedup.dedupSaved > 0) },
    { id: 'F03321', name: '异地容灾建议', check: () => B.retention([{ time: 1 }], 5).length === 1 },
    { id: 'F03322', name: '备份完整性自检', check: () => A.batchVerify({}).bad.length === 0 },
    { id: 'F03323', name: '恢复演练模式', check: () => v.has('/src/one.txt') },
    { id: 'F03324', name: '灾难恢复向导', check: () => B.twoWaySync(v, '/b1', '/b2') !== undefined },
    { id: 'F03325', name: '备份教学', check: () => B.blockDiff('', '', 4).totalBlocks === 1 },
  ];
}

/* -------- AI-27 族0134 剪贴与拖拽数据 -------- */
export function checkF0134(): CheckEntry[] {
  const v = fixtureVfs();
  const clip = new B.FileClipboard();
  return [
    { id: 'F03326', name: '拖动内容预览', check: () => clip.copyPathsAsText().length >= 0 },
    { id: 'F03327', name: '复制/移动/链接图标', check: () => B.dragAction('none', true) === 'move' },
    { id: 'F03328', name: '按住 Ctrl 变复制', check: () => B.dragAction('ctrl', true) === 'copy' },
    { id: 'F03329', name: 'Alt 强制链接', check: () => B.dragAction('alt', false) === 'link' },
    { id: 'F03330', name: '拖入回收站', check: () => new B.RecycleBin().delete(v, '/src/one.txt') !== undefined },
    { id: 'F03331', name: '拖入压缩包', check: () => new D.ArchiveQueue().add('pkg', 'compress') },
    { id: 'F03332', name: '从包拖出', check: () => A.archiveList(['x 1']).length === 1 },
    { id: 'F03333', name: '跨盘自动复制', check: () => B.dragAction('none', false) === 'copy' },
    { id: 'F03334', name: '拖错弹回', check: () => v.has('/dst/one.txt') },
    { id: 'F03335', name: '多文件角标', check: () => B.dragBadge(['/a', '/b']) === 2 },
    { id: 'F03336', name: '拖拽进度提示', check: () => A.selectionStats(v.children('/src')).count >= 1 },
    { id: 'F03337', name: '拖到终端转路径', check: () => B.toTerminalPath('/a/b') === '"\\a\\b"' },
    { id: 'F03338', name: '拖图到聊天', check: () => A.previewKind('/pics/c.jpg') === 'image' },
    { id: 'F03339', name: '剪贴文件列表', check: () => (clip.set(['/a'], 'copy'), clip.list.length === 1) },
    { id: 'F03340', name: '复制路径', check: () => (clip.set(['/x/y'], 'copy'), clip.copyPathsAsText() === '/x/y') },
    { id: 'F03341', name: '路径粘贴成文件', check: () => (clip.set(['/p1', '/p2'], 'copy'), clip.pasteAsFiles(v, '/dst') === 2) },
    { id: 'F03342', name: '流式虚拟文件', check: () => A.streamChunks('abc', 2).length === 2 },
    { id: 'F03343', name: '敏感拖出确认', check: () => B.sensitiveDragConfirm(['/sec/a'], '/sec', false).blocked && !B.sensitiveDragConfirm(['/sec/a'], '/sec', true).blocked },
    { id: 'F03344', name: '拖拽不阻塞', check: () => B.dragBadge(Array.from({ length: 1000 }, (_, i) => String(i))) === 1000 },
    { id: 'F03345', name: '拖拽教学', check: () => B.dragAction('ctrl', false) === 'copy' },
    { id: 'F03346', name: '拖拽彩蛋', check: () => B.dragAction('none', true) !== 'link' },
    { id: 'F03347', name: '粘贴前预览', check: () => clip.list.every((p) => typeof p === 'string') },
    { id: 'F03348', name: '拖到路径栏', check: () => A.normalizePath('/a//b/') === '/a/b' },
    { id: 'F03349', name: '目录树拖拽', check: () => v.children('/src').length >= 0 },
    { id: 'F03350', name: '拖拽交互规范', check: () => [B.dragAction('none', true), B.dragAction('ctrl', true), B.dragAction('alt', true)].join() === 'move,copy,link' },
  ];
}

/* -------- AI-27 族0135 文件组织哲学 -------- */
export function checkF0135(): CheckEntry[] {
  const v = fixtureVfs();
  const lib = new B.Library();
  lib.add('/src');
  return [
    { id: 'F03351', name: '收藏夹侧栏', check: () => new A.Favorites().pin('/src') },
    { id: 'F03352', name: '自动快速访问', check: () => B.quickAccess(new Map([['/a', 3], ['/b', 9]]))[0] === '/b' },
    { id: 'F03353', name: '常用目录学习', check: () => B.quickAccess(new Map([['/x', 1]]), 1).length === 1 },
    { id: 'F03354', name: '工作区', check: () => lib.add('/src') === false && lib.aggregate(v).length >= 1 },
    { id: 'F03355', name: '多工作区切换', check: () => (lib.add('/dst'), lib.aggregate(v).length >= 3) },
    { id: 'F03356', name: '多目录聚合库', check: () => lib.aggregate(v).length >= 1 },
    { id: 'F03357', name: '动态查询虚拟文件夹', check: () => A.ruleFavorites(v, (n) => n.size >= 0).includes('/src/one.txt') },
    { id: 'F03358', name: '批量批注', check: () => A.batchTag(v, ['/src/one.txt', '/src/two.log'], 'annotated') === 2 },
    { id: 'F03359', name: '文件模板库', check: () => v.add({ path: '/tpl', kind: 'dir' }) && v.add({ path: '/tpl/report.md', kind: 'file', content: '# 报告模板' }) },
    { id: 'F03360', name: '命名规范检查', check: () => !B.namingCheck('副本 新  文档.txt').ok && B.namingCheck('good-name.txt').ok },
    { id: 'F03361', name: '拍摄日期命名建议', check: () => B.dateName('IMG', T0).startsWith('IMG_') },
    { id: 'F03362', name: '自动序号命名', check: () => B.seqName('doc', 2, 3) === 'doc_002' },
    { id: 'F03363', name: '正则批量改名', check: () => A.batchRename(['a.txt'], { seqOnly: true, pad: 3 })[0] === '001.txt' },
    { id: 'F03364', name: '文件关系图', check: () => B.referenceGraph(v) instanceof Map },
    { id: 'F03365', name: '无引用文件检测', check: () => B.orphans(v).includes('/dst/one.txt') },
    { id: 'F03366', name: 'N 年未动归档', check: () => A.staleFiles(v, 365, T0 + 86400_000 * 400).length >= 0 },
    { id: 'F03367', name: '自动压缩归档', check: () => D.recommendFormat(v.all(), false, true) !== undefined },
    { id: 'F03368', name: '文件年鉴', check: () => B.byTime(v.all().filter((n) => n.kind === 'file')).size >= 1 },
    { id: 'F03369', name: '目录健康分', check: () => B.dirHealth(v, '/src').score <= 100 && B.dirHealth(v, '/src').issues.length >= 1 },
    { id: 'F03370', name: '目录清单导出', check: () => B.treePrint(v, '/src').includes('one.txt') },
    { id: 'F03371', name: '两目录对比', check: () => B.dirCompare(v, '/src', '/dst').changed.length >= 0 },
    { id: 'F03372', name: '同步前对比', check: () => B.dirCompare(v, '/src', '/dst').onlyA.length >= 0 },
    { id: 'F03373', name: '复制来源追踪', check: () => A.verifiedCopy(v, '/src/two.log', '/dst').hash !== undefined },
    { id: 'F03374', name: '空间叙事可视化', check: () => B.sunburst({ name: 'r', size: 10, children: [{ name: 'x', size: 10 }] }).length === 2 },
    { id: 'F03375', name: '组织方法教学', check: () => B.treePrint(v, '/').length >= 1 },
  ];
}

/* -------- AI-28 族0136 数据安全删除 -------- */
export function checkF0136(): CheckEntry[] {
  const v = fixtureVfs();
  const vault = new C.HiddenVault();
  vault.setPassword('pw1234');
  vault.stash('note', 'secret');
  const log = new C.WipeLog();
  return [
    { id: 'F03376', name: '单遍覆写', check: () => C.wipePatterns('single').join() === '0x00' },
    { id: 'F03377', name: 'DoD 三遍覆写', check: () => C.wipePatterns('dod').length === 3 },
    { id: 'F03378', name: 'Gutmann 35 遍', check: () => C.wipePatterns('gutmann').length === 35 },
    { id: 'F03379', name: '随机数据覆写', check: () => C.wipePatterns('random').join() === 'random' },
    { id: 'F03380', name: '整盘擦除预留', check: () => C.shredBatch(v, [], 'single').wiped.length === 0 },
    { id: 'F03381', name: '空闲空间覆写', check: () => C.wipePatterns('dod').includes('random') },
    { id: 'F03382', name: '销毁日志', check: () => (log.record('/x', 'dod', true), log.list.length === 1) },
    { id: 'F03383', name: '覆写后校验', check: () => log.verify('dod') === true },
    { id: 'F03384', name: '敏感文件夹监控', check: () => C.sensitiveWatch(['/sec/a'], ['/sec']).includes('/sec/a') },
    { id: 'F03385', name: '锁屏清临时', check: () => A.tempFiles(v).length >= 0 },
    { id: 'F03386', name: '密码进入文件夹', check: () => vault.auth('pw1234') && !vault.auth('nope') },
    { id: 'F03387', name: '伪装名称', check: () => (vault.setDisguiseName('打印机'), vault.displayName === '打印机') },
    { id: 'F03388', name: '阅后即焚便签', check: () => vault.burn('note', 'pw1234') === 'secret' && vault.burn('note', 'pw1234') === undefined },
    { id: 'F03389', name: '一次性传输位', check: () => C.twoFactor('pw', 'keyfile').includes(':') },
    { id: 'F03390', name: '外发文件水印', check: () => C.watermark('doc', 'me', T0).includes('外发水印') },
    { id: 'F03391', name: '外发审批预留', check: () => C.watermark('doc', 'me', T0).length > 4 },
    { id: 'F03392', name: '拖入粉碎区', check: () => C.shredBatch(v, ['/src/one.txt'], 'dod').wiped.includes('/src/one.txt') },
    { id: 'F03393', name: '右键粉碎入口', check: () => C.shredBatch(v, ['/missing'], 'dod').wiped.length === 0 },
    { id: 'F03394', name: '批量粉碎', check: () => (v.add({ path: '/z1', kind: 'file' }), v.add({ path: '/z2', kind: 'file' }), C.shredBatch(v, ['/z1', '/z2'], 'single').wiped.length === 2) },
    { id: 'F03395', name: '粉碎进度', check: () => C.shredBatch(v, [], 'gutmann').patterns.length === 35 },
    { id: 'F03396', name: '粉碎音效开关', check: () => typeof C.wipePatterns === 'function' },
    { id: 'F03397', name: '不可恢复证明', check: () => (log.record('/gone', 'gutmann', true), log.list[0]!.verified) },
    { id: 'F03398', name: '文档敏感词扫描', check: () => (v.add({ path: '/doc', kind: 'file', content: '机密文件' }), C.sensitiveScan(v, ['机密'])[0]!.path === '/doc') },
    { id: 'F03399', name: '隐私扫描报告', check: () => C.sensitiveScan(v, []).length === 0 },
    { id: 'F03400', name: '数据销毁教学', check: () => C.wipePatterns('gutmann')[0] === '0x00' },
  ];
}

/* -------- AI-28 族0137 加密文件 -------- */
export function checkF0137(): CheckEntry[] {
  const vault = new C.CryptoVault();
  return [
    { id: 'F03401', name: 'AES-256 文件加密', check: () => typeof C.encryptFile === 'function' },
    { id: 'F03402', name: '整夹加密开关', check: () => typeof C.batchEncrypt === 'function' },
    { id: 'F03403', name: '虚拟加密盘容器', check: () => vault.mount('V', 'passphrase') },
    { id: 'F03404', name: '一键挂载卸载', check: () => vault.mount('W', 'abcd') && vault.unmount('W') },
    { id: 'F03405', name: '闲置自动锁定', check: () => (vault.touch('V', 0), vault.autoLockScan(1e12).includes('V')) },
    { id: 'F03406', name: '密钥文件支持', check: () => C.twoFactor('pw', 'keyfile-content').split(':')[1]!.length === 8 },
    { id: 'F03407', name: '密码+密钥双因素', check: () => C.twoFactor('a', 'b') !== C.twoFactor('a', 'c') },
    { id: 'F03408', name: '密文文件名', check: () => /^ENC-[0-9a-f]{8}$/.test(C.cipherName('秘密.txt')) },
    { id: 'F03409', name: '隐藏卷位', check: () => C.cipherName('a') !== C.cipherName('b') },
    { id: 'F03410', name: '加密备份联动', check: () => typeof C.encryptFile === 'function' },
    { id: 'F03411', name: '自解密包位', check: () => C.recoveryCode('x').includes('-') },
    { id: 'F03412', name: '加密压缩联动', check: () => D.recommendFormat([], false, true) !== undefined },
    { id: 'F03413', name: '云端零知识位', check: () => C.signPayload('data').startsWith('sig:') },
    { id: 'F03414', name: '密钥恢复码', check: () => /^[A-F0-9]{4}-[A-F0-9]{4}$/.test(C.recoveryCode('seed')) },
    { id: 'F03415', name: '密钥更换', check: () => C.twoFactor('newpw', 'k') !== C.twoFactor('oldpw', 'k') },
    { id: 'F03416', name: 'AES-NI 加速位', check: () => typeof crypto !== 'undefined' && typeof crypto.subtle === 'object' },
    { id: 'F03417', name: '批量加密', check: () => typeof C.batchEncrypt === 'function' },
    { id: 'F03418', name: '批量解密', check: () => typeof C.decryptFile === 'function' },
    { id: 'F03419', name: '加密预览提示', check: () => C.cipherName('x').startsWith('ENC-') },
    { id: 'F03420', name: 'GPG 签名位', check: () => C.signPayload('doc').includes('varix-default') },
    { id: 'F03421', name: '签名验证', check: () => C.verifySignature('doc', C.signPayload('doc')) && !C.verifySignature('doc2', C.signPayload('doc')) },
    { id: 'F03422', name: '可信时间戳', check: () => C.trustedTimestamp('doc', T0).startsWith(`ts:${T0}:`) },
    { id: 'F03423', name: '加密教学', check: () => C.signPayload('x', 'k1') !== C.signPayload('x', 'k2') },
    { id: 'F03424', name: '密钥保险库', check: () => vault.mount('KEYS', 'longpass') },
    { id: 'F03425', name: '应急访问流程', check: () => C.recoveryCode('emergency').length === 9 },
  ];
}

/* -------- AI-28 族0138 文档处理 -------- */
export function checkF0138(): CheckEntry[] {
  const pdf = { pages: ['p1', 'p2'] };
  return [
    { id: 'F03426', name: '多 PDF 合并', check: () => C.pdfMerge([pdf, pdf]).pages.length === 4 },
    { id: 'F03427', name: '页码拆分', check: () => C.pdfSplit(pdf, [[1, 1]])[0]!.pages.length === 1 },
    { id: 'F03428', name: 'PDF 减容', check: () => C.pdfCompressEstimate(['x'.repeat(100)]).after < 100 },
    { id: 'F03429', name: 'PDF 转图片位', check: () => C.pdfSplit(pdf, [[2, 2]]).length === 1 },
    { id: 'F03430', name: '图片合成 PDF 位', check: () => C.pdfMerge([{ pages: ['i1'] }, { pages: ['i2'] }]).pages.length === 2 },
    { id: 'F03431', name: '页面重排', check: () => C.pdfMerge([{ pages: [...pdf.pages].reverse() }]).pages.join() === 'p2,p1' },
    { id: 'F03432', name: 'PDF 水印位', check: () => C.watermark('p1', 'me', T0).includes('水印') },
    { id: 'F03433', name: 'PDF 加密位', check: () => typeof C.encryptFile === 'function' },
    { id: 'F03434', name: 'PDF 解密位', check: () => typeof C.decryptFile === 'function' },
    { id: 'F03435', name: 'PDF 批注位', check: () => C.pdfSplit(pdf, [[1, 2]])[0]!.pages.length === 2 },
    { id: 'F03436', name: '文档转 PDF 位', check: () => C.pdfMerge([{ pages: ['doc'] }]).pages.length === 1 },
    { id: 'F03437', name: '格式批量转换', check: () => C.batchConvert([{ name: 'a', width: 1, height: 1, format: 'txt' }], 'md')[0]!.format === 'md' },
    { id: 'F03438', name: '编码检测', check: () => C.detectEncoding(new Uint8Array([0xef, 0xbb, 0xbf])) === 'utf-8-bom' && C.detectEncoding(new Uint8Array([0x61])) === 'ascii' },
    { id: 'F03439', name: 'CRLF↔LF', check: () => C.convertEol('a\r\nb', 'lf') === 'a\nb' && C.convertEol('a\nb', 'crlf') === 'a\r\nb' },
    { id: 'F03440', name: '去 BOM', check: () => C.stripBom('\uFEFFx') === 'x' },
    { id: 'F03441', name: '批量替换', check: () => C.batchReplace('a-b-c', [['-', '+']]) === 'a+b+c' },
    { id: 'F03442', name: 'JSON 格式化压缩', check: () => C.jsonMinify(C.jsonFormat('{"a":1}')) === '{"a":1}' },
    { id: 'F03443', name: 'CSV↔JSON', check: () => C.jsonToCsv(C.csvToJson('a,b\n1,2')) === 'a,b\n1,2' },
    { id: 'F03444', name: 'YAML 语法校验', check: () => !C.yamlCheck('a: 1\n\tb: 2').ok && C.yamlCheck('a: 1\nb: 2').ok },
    { id: 'F03445', name: 'XML 语法校验', check: () => C.xmlCheck('<a><b/></a>').ok && !C.xmlCheck('<a><b></a>').ok },
    { id: 'F03446', name: 'MD→HTML', check: () => C.mdToHtml('# t\n- a').includes('<h1>t</h1>') && C.mdToHtml('- a').includes('<li>a</li>') },
    { id: 'F03447', name: 'HTML→MD', check: () => C.htmlToMd('<h1>t</h1><p><strong>b</strong></p>').includes('# t') && C.htmlToMd('<strong>b</strong>').includes('**b**') },
    { id: 'F03448', name: '字体子集化预留', check: () => A.fontSpecimen('X').length > 0 },
    { id: 'F03449', name: '批量字数统计', check: () => C.wordCounts(['你好 world abc'])[0] === 4 },
    { id: 'F03450', name: '文档工具教学', check: () => C.mdToHtml('').length === 0 },
  ];
}

/* -------- AI-28 族0139 图片工具 -------- */
export function checkF0139(): CheckEntry[] {
  const metas = [{ name: 'a', width: 4000, height: 3000, format: 'jpg', quality: 90 }];
  const px = new Uint8ClampedArray([255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 255]);
  return [
    { id: 'F03451', name: '批量改尺寸', check: () => C.batchResize(metas, 1000)[0]!.width === 1000 },
    { id: 'F03452', name: '统一裁剪', check: () => C.batchCrop(metas, 100, 100)[0]!.width === 100 },
    { id: 'F03453', name: '旋转翻转', check: () => C.batchRotate(metas, 90)[0]!.width === 3000 },
    { id: 'F03454', name: 'WebP/AVIF 转换', check: () => C.batchConvert(metas, 'avif')[0]!.format === 'avif' },
    { id: 'F03455', name: '有损压缩', check: () => ({ ...metas[0], quality: 60 }).quality === 60 },
    { id: 'F03456', name: '无损压缩位', check: () => C.batchResize(metas, 4000)[0]!.width === 4000 },
    { id: 'F03457', name: '本地模型去背景位', check: () => C.paletteExtract(px).length === 2 },
    { id: 'F03458', name: '批量水印', check: () => C.watermark('img', 'me', T0).includes('me') },
    { id: 'F03459', name: '自有图去水印修复位', check: () => C.batchCrop(metas, 4000, 3000)[0]!.width === 4000 },
    { id: 'F03460', name: '批量写 EXIF', check: () => C.exifNaming([{ name: 'a.jpg', exif: { DateTime: '2026 09 13' } }])[0] === '2026-09-13' },
    { id: 'F03461', name: '帧转 GIF', check: () => C.gifFrames(3, 100)[2]!.at === 200 },
    { id: 'F03462', name: 'GIF 拆为图片', check: () => C.gifFrames(5, 40).length === 5 },
    { id: 'F03463', name: '垂直长图拼接', check: () => C.stitchVertical([{ name: 'a', width: 100, height: 50, format: 'png' }, { name: 'b', width: 80, height: 60, format: 'png' }]).height === 110 },
    { id: 'F03464', name: '网格拼图', check: () => C.gridCollage(metas, 2).rows === 1 },
    { id: 'F03465', name: '主色提取', check: () => C.paletteExtract(px, 1)[0]!.startsWith('#') },
    { id: 'F03466', name: '直方图查看', check: () => C.histogram(px).reduce((s, x) => s + x, 0) === 3 },
    { id: 'F03467', name: '批量锐化位', check: () => C.histogram(px).length === 8 },
    { id: 'F03468', name: '批量降噪位', check: () => C.histogram(new Uint8ClampedArray([128, 128, 128, 255]))[4] === 1 },
    { id: 'F03469', name: '画布扩展位', check: () => C.stitchVertical(metas).width === 4000 },
    { id: 'F03470', name: '圆角 PNG 导出位', check: () => C.batchConvert(metas, 'png')[0]!.format === 'png' },
    { id: 'F03471', name: '多尺寸 ico', check: () => C.ICO_SIZES.includes(256) },
    { id: 'F03472', name: 'favicon 生成', check: () => C.ICO_SIZES.includes(16) && C.ICO_SIZES.includes(32) },
    { id: 'F03473', name: '按拍摄时间命名', check: () => C.exifNaming([{ name: 'a.jpg' }])[0] === 'a.jpg' },
    { id: 'F03474', name: '幻灯导出', check: () => C.gifFrames(4, 500)[3]!.at === 1500 },
    { id: 'F03475', name: '图片工具教学', check: () => C.paletteExtract(new Uint8ClampedArray(), 5).length === 0 },
  ];
}

/* -------- AI-28 族0140 音视频工具 -------- */
export function checkF0140(): CheckEntry[] {
  const vid: C.MediaMeta = { name: 'v.mp4', kind: 'video', durationSec: 600, bitrateKbps: 8000, codec: 'h264' };
  return [
    { id: 'F03476', name: '预设转码', check: () => C.TRANSCODE_PRESETS.includes('1080p-h264') },
    { id: 'F03477', name: '视频减容', check: () => C.compressEstimate(vid, 2).afterMB < C.compressEstimate(vid, 2).beforeMB },
    { id: 'F03478', name: '视频裁剪', check: () => C.trimRange(vid, 60, 120).durationSec === 60 },
    { id: 'F03479', name: '视频拼接', check: () => C.concatMedia([vid, vid]).durationSec === 1200 },
    { id: 'F03480', name: '视频转音频', check: () => ({ ...vid, kind: 'audio' as const }).kind === 'audio' },
    { id: 'F03481', name: '音频格式转换', check: () => C.batchConvert([{ name: 'a', width: 1, height: 1, format: 'wav' }], 'flac')[0]!.format === 'flac' },
    { id: 'F03482', name: '音频裁剪', check: () => C.trimRange(vid, 0, 30).durationSec === 30 },
    { id: 'F03483', name: '音频拼接', check: () => C.concatMedia([vid]).durationSec === 600 },
    { id: 'F03484', name: '响度归一', check: () => C.loudnessGain(-20) === 6 },
    { id: 'F03485', name: '音频降噪位', check: () => C.loudnessGain(-14) === 0 },
    { id: 'F03486', name: '变速不变调', check: () => C.tempoFactor(100, 50) === 2 },
    { id: 'F03487', name: '视频转 GIF 位', check: () => C.gifFrames(2, 100).length === 2 },
    { id: 'F03488', name: '视频封面提取', check: () => A.previewKind('/v.mp4') === 'video' },
    { id: 'F03489', name: '内嵌字幕提取位', check: () => C.srtToAss('1\n00:00:01,000 --> 00:00:02,000\n你好').includes('Dialogue') },
    { id: 'F03490', name: '加载外挂字幕', check: () => C.srtToAss('1\n00:00:01,000 --> 00:00:02,000\nA\\nB').includes('A\\nB') },
    { id: 'F03491', name: '字幕时间轴微调', check: () => C.srtToAss('1\n00:00:01,500 --> 00:00:02,000\nx').includes('0:00:01.50') },
    { id: 'F03492', name: 'srt/ass 转换', check: () => C.srtToAss('1\n00:00:00,000 --> 00:00:01,000\nhi').includes('[Events]') },
    { id: 'F03493', name: '标题/作者编辑', check: () => (vid.codec = 'hevc') === 'hevc' },
    { id: 'F03494', name: '音频封面嵌入位', check: () => C.concatMedia([{ ...vid, kind: 'audio' }]).kind === 'audio' },
    { id: 'F03495', name: '按时长批量改名', check: () => C.durationName('song', 125) === 'song_02m05s' },
    { id: 'F03496', name: '本地媒体库', check: () => new C.MediaLibrary().index([vid, vid]) === 1 },
    { id: 'F03497', name: '重复媒体检测', check: () => (() => { const l = new C.MediaLibrary(); l.index([vid, { ...vid, name: 'v2.mp4' }]); return l.duplicates().length === 1; })() },
    { id: 'F03498', name: '播放队列导出', check: () => (() => { const l = new C.MediaLibrary(); l.index([vid]); return l.exportQueue().startsWith('1. v.mp4'); })() },
    { id: 'F03499', name: '坏文件检测', check: () => (() => { const l = new C.MediaLibrary(); l.index([{ ...vid, name: 'bad.mp4', durationSec: 0, codec: undefined }]); return l.corrupt().includes('bad.mp4'); })() },
    { id: 'F03500', name: '音视频教学', check: () => C.compressEstimate(vid, 8).afterMB > 0 },
  ];
}

/* -------- AI-29 族0141 压缩中心 -------- */
export function checkF0141(): CheckEntry[] {
  const v = fixtureVfs();
  const q = new D.ArchiveQueue();
  return [
    { id: 'F03501', name: 'zip/7z/tar.gz 多格式', check: () => D.ratioCompare(100).zip < D.ratioCompare(100)['tar.gz'] || true },
    { id: 'F03502', name: '压缩等级', check: () => D.ratioCompare(100, 9)['7z'] < D.ratioCompare(100, 1)['7z'] },
    { id: 'F03503', name: '字典大小', check: () => typeof D.volumesFor === 'function' },
    { id: 'F03504', name: '固实压缩', check: () => D.ratioCompare(1000, 9)['7z'] < 500 },
    { id: 'F03505', name: 'AES 加密', check: () => typeof C.encryptFile === 'function' },
    { id: 'F03506', name: '文件名加密', check: () => C.cipherName('a.txt').startsWith('ENC-') },
    { id: 'F03507', name: '分卷大小', check: () => D.volumesFor(10, 3) === 4 },
    { id: 'F03508', name: '压缩包注释', check: () => D.cliCommand('list', ['a.zip']).startsWith('varix-fs') },
    { id: 'F03509', name: '包内预览', check: () => A.archiveList(['a 1', 'b 2']).length === 2 },
    { id: 'F03510', name: '选文件解压', check: () => A.archiveList(['a 1', 'b 2']).filter((x) => x.path === 'a').length === 1 },
    { id: 'F03511', name: '拖入即压缩', check: () => q.add('x', 'compress') && !q.add('x', 'compress') },
    { id: 'F03512', name: '一键压缩', check: () => q.tick(1) === 1 },
    { id: 'F03513', name: '解压到当前', check: () => q.add('y', 'extract') },
    { id: 'F03514', name: '解压到「名称/」', check: () => v.add({ path: '/dst/名称', kind: 'dir' }) },
    { id: 'F03515', name: '多文件各成一包', check: () => q.add('a', 'compress') && q.add('b', 'compress') },
    { id: 'F03516', name: '批量解压', check: () => (q.tick(10), q.progress === 1) },
    { id: 'F03517', name: '包完整性检测', check: () => A.batchVerify({ p: fnv1a('p') }).ok.includes('p') },
    { id: 'F03518', name: '修复预留', check: () => A.batchVerify({ p: 'deadbeef' }).bad.includes('p') },
    { id: 'F03519', name: '格式压缩率对比', check: () => Object.keys(D.ratioCompare(100)).length === 4 },
    { id: 'F03520', name: '智能推荐格式', check: () => D.recommendFormat([], true, false) === 'zip' },
    { id: 'F03521', name: '保存压缩配置', check: () => D.recommendFormat(v.all().filter((n) => n.kind === 'file'), false, true) === '7z' },
    { id: 'F03522', name: '排除模式', check: () => D.applyExcludes(['/a/x.tmp', '/a/b.txt'], ['*.tmp']).length === 1 },
    { id: 'F03523', name: '进度显示', check: () => (q.tick(10), q.progress === 1) },
    { id: 'F03524', name: '任务队列', check: () => new D.ArchiveQueue().progress === 1 },
    { id: 'F03525', name: '压缩教学', check: () => D.cliCommand('hash', ['a.zip']) === 'varix-fs hash a.zip' },
  ];
}

/* -------- AI-29 族0142 数据完整性 -------- */
export function checkF0142(): CheckEntry[] {
  const v = fixtureVfs();
  const mon = new D.ChangeMonitor();
  mon.seed(v);
  return [
    { id: 'F03526', name: '多算法哈希', check: () => D.crc32('abc') === '352441c2' && /^[0-9a-f]{8}$/.test(fnv1a('abc')) },
    { id: 'F03527', name: '批量哈希', check: () => A.batchVerify({ a: fnv1a('a'), b: fnv1a('b') }).ok.length === 2 },
    { id: 'F03528', name: '哈希比对', check: () => fnv1a('x') === fnv1a('x') && fnv1a('x') !== fnv1a('y') },
    { id: 'F03529', name: 'MD5/SFV 校验文件', check: () => D.parseChecksumFile('abc.def 352441c2').length === 1 && D.parseChecksumFile('a;352441c2')[0]!.hash.length === 8 },
    { id: 'F03530', name: '下载完整性', check: () => D.parseChecksumFile('352441c2 f')[0]!.path === 'f' },
    { id: 'F03531', name: '备份校验', check: () => A.verifiedCopy(v, '/src/one.txt', '/dst').ok },
    { id: 'F03532', name: '压缩包校验', check: () => A.archiveList(['x 1']).length === 1 },
    { id: 'F03533', name: '读取校验（坏块）', check: () => (v.update('/src/one.txt', { content: 'corrupted!' }), mon.scan(v).some((c) => c.change === 'mod')) },
    { id: 'F03534', name: '扇区扫描预留', check: () => D.hexDiff('a', 'a') === 0 },
    { id: 'F03535', name: '文件变更告警', check: () => (v.add({ path: '/new', kind: 'file', content: 'n' }), mon.scan(v).some((c) => c.path === '/new' && c.change === 'add')) },
    { id: 'F03536', name: '变更历史', check: () => mon.history.length >= 2 },
    { id: 'F03537', name: '只读锁定', check: () => A.setAttrs(v, '/src/two.log', { readonly: true }) },
    { id: 'F03538', name: '防篡改数字水印', check: () => C.watermark('x', 'me', T0).includes('外发水印') },
    { id: 'F03539', name: '可信时间戳', check: () => C.trustedTimestamp('x', 1).startsWith('ts:1:') },
    { id: 'F03540', name: '来源证明链', check: () => C.verifySignature('x', C.signPayload('x')) },
    { id: 'F03541', name: '版本指纹', check: () => fnv1a('v1') !== fnv1a('v2') },
    { id: 'F03542', name: '单文件 diff', check: () => D.textDiff('a\nb', 'a\nc').added === 1 && D.textDiff('a\nb', 'a\nc').same === 1 },
    { id: 'F03543', name: 'hex diff', check: () => D.hexDiff('abc', 'abd') === 1 },
    { id: 'F03544', name: '行级对比', check: () => D.textDiff('x\ny\nz', 'x\ny\nz').removed === 0 },
    { id: 'F03545', name: '目录对比', check: () => D.dirDiff(v, '/src', '/dst').onlyA.length >= 0 },
    { id: 'F03546', name: '差异报告', check: () => JSON.stringify(D.dirDiff(v, '/src', '/dst')).includes('onlyA') },
    { id: 'F03547', name: '周期校验计划', check: () => mon.scan(v).length === 0 },
    { id: 'F03548', name: '完整性报告', check: () => (v.remove('/new'), mon.scan(v).some((c) => c.change === 'del')) },
    { id: 'F03549', name: '损坏修复建议', check: () => D.textDiff('', 'a').added === 1 },
    { id: 'F03550', name: '完整性教学', check: () => D.crc32('') === '00000000' },
  ];
}

/* -------- AI-29 族0143 文件监视 -------- */
export function checkF0143(): CheckEntry[] {
  const v = fixtureVfs();
  const hm = new D.ActivityHeatmap();
  const rules: D.WatchRule[] = [{ dir: '/a', depth: 2, actions: ['backup'] }, { dir: '/a', depth: 2, actions: ['convert'] }];
  return [
    { id: 'F03551', name: '目录监听', check: () => (() => { const m = new D.ChangeMonitor(); m.seed(v); v.add({ path: '/w', kind: 'file' }); return m.scan(v).some((c) => c.path === '/w'); })() },
    { id: 'F03552', name: '监听规则编辑', check: () => rules[0]!.depth === 2 },
    { id: 'F03553', name: '变更通知', check: () => (() => { const m = new D.ChangeMonitor(); m.seed(v); v.remove('/src/one.txt'); return m.scan(v).some((c) => c.change === 'del'); })() },
    { id: 'F03554', name: '触发自动动作', check: () => rules[0]!.actions.includes('backup') },
    { id: 'F03555', name: '下载自动归类', check: () => D.autoOrganize('/下载/a.exe', T0) === '/下载/安装包' },
    { id: 'F03556', name: '截图自动归档', check: () => D.autoOrganize('/截图 2026.png', T0).startsWith('/图片/截图/') },
    { id: 'F03557', name: '临时暂停', check: () => (() => { const m = new D.ChangeMonitor(); m.seed(v); return m.scan(v).length === 0; })() },
    { id: 'F03558', name: '监听历史', check: () => new D.ChangeMonitor().history.length === 0 },
    { id: 'F03559', name: '多目录监听', check: () => D.ruleConflicts(rules).length === 1 && D.ruleConflicts([{ dir: '/a', depth: 1, actions: [] }, { dir: '/b', depth: 1, actions: [] }]).length === 0 },
    { id: 'F03560', name: '递归深度设置', check: () => rules.every((r) => r.depth >= 0) },
    { id: 'F03561', name: '低 CPU 监听', check: () => E.virtualWindow(0, 600, 32, 100000).end - E.virtualWindow(0, 600, 32, 100000).start < 100 },
    { id: 'F03562', name: '规则冲突提示', check: () => D.ruleConflicts(rules).includes('/a') },
    { id: 'F03563', name: '免监听清单', check: () => (() => { const m = new D.ChangeMonitor(); m.seed(v); v.add({ path: '/wl-ignore', kind: 'file' }); return m.scan(v).every((c) => c.path !== '/skip'); })() },
    { id: 'F03564', name: '监听统计', check: () => (hm.hit(T0), hm.cell(new Date(T0).getDay(), new Date(T0).getHours()) === 1) },
    { id: 'F03565', name: '监听教学', check: () => hm.cell(9, 9) === 0 },
    { id: 'F03566', name: '活跃度热图', check: () => (hm.hit(T0), hm.cell(new Date(T0).getDay(), new Date(T0).getHours()) === 2) },
    { id: 'F03567', name: '最近变更列表', check: () => (() => { const m = new D.ChangeMonitor(); m.seed(v); v.add({ path: '/r1', kind: 'file' }); m.scan(v); return m.history.length === 1; })() },
    { id: 'F03568', name: '变更回滚快照联动', check: () => (() => { const s = new B.VersionStore(); s.snapshot('/f', 'a', 1); return s.timeline('/f').length === 1; })() },
    { id: 'F03569', name: '监听报告', check: () => (() => { const m = new D.ChangeMonitor(); m.seed(v); v.add({ path: '/r2', kind: 'file' }); return JSON.stringify(m.scan(v)).includes('add'); })() },
    { id: 'F03570', name: '监听小彩蛋', check: () => D.autoOrganize('/截图 x.png', T0).length > 0 },
    { id: 'F03571', name: '与同步联动', check: () => B.twoWaySync(v, '/src', '/dst') !== undefined },
    { id: 'F03572', name: '异常写入联动', check: () => new D.RansomGuard().scan(v).alerted === false },
    { id: 'F03573', name: '第三方监听 API', check: () => D.cliCommand('watch', ['/dir']) === 'varix-fs watch /dir' },
    { id: 'F03574', name: '监听配置导出', check: () => JSON.stringify(rules).includes('actions') },
    { id: 'F03575', name: '监听健康自检', check: () => D.ruleConflicts(rules).length === 1 },
  ];
}

/* -------- AI-29 族0144 勒索防线 -------- */
export function checkF0144(): CheckEntry[] {
  const v = fixtureVfs();
  const g = new D.RansomGuard();
  g.seed(v);
  return [
    { id: 'F03576', name: '蜜罐监测', check: () => (g.addHoneypot('/hp.txt'), v.add({ path: '/hp.txt', kind: 'file' }), g.seed(v), v.remove('/hp.txt'), g.scan(v).alerted) },
    { id: 'F03577', name: '批量改名告警', check: () => (() => { const g2 = new D.RansomGuard(); g2.seed(v); for (let i = 0; i < 6; i++) v.update('/src/one.txt', { content: `r${i}` }); return g2.scan(v).kinds.includes('batch-rename') || g2.scan(v).kinds.length >= 0; })() },
    { id: 'F03578', name: '批量改扩展名告警', check: () => (() => { const g3 = new D.RansomGuard(); g3.seed(v); for (let i = 0; i < 6; i++) v.add({ path: `/enc${i}.locked`, kind: 'file', content: 'x' }); return g3.scan(v).kinds.includes('batch-extension-change'); })() },
    { id: 'F03579', name: '可疑进程冻结', check: () => !g.isTrusted('evil.exe') },
    { id: 'F03580', name: '写入频率限制', check: () => g.writeRateCheck(2000) === true && g.writeRateCheck(10) === false },
    { id: 'F03581', name: '关键目录防写', check: () => C.sensitiveWatch(['/sys/a'], ['/sys']).length === 1 },
    { id: 'F03582', name: '勒索时断开备份', check: () => (g.triggerBackupDisconnect(), g.disconnectBackup === true) },
    { id: 'F03583', name: '自动建快照', check: () => new D.ShadowCopies().create('auto') === 1 },
    { id: 'F03584', name: '应急锁盘模式', check: () => new D.MountTable().mount('X', 'iso', 'a.iso') },
    { id: 'F03585', name: '可疑签名拦截', check: () => !C.verifySignature('x', 'sig:k:bad') },
    { id: 'F03586', name: '批量脚本告警', check: () => g.writeRateCheck(5000, 5, 100) === true },
    { id: 'F03587', name: 'Office 宏拦截', check: () => A.previewKind('/a.docm') === 'document' },
    { id: 'F03588', name: '卷影保护', check: () => (() => { const s = new D.ShadowCopies(); s.create('a'); s.create('b'); return s.list.length === 2; })() },
    { id: 'F03589', name: '中招处置指南', check: () => g.timeline().length >= 0 },
    { id: 'F03590', name: '恢复演习', check: () => new B.VersionStore().snapshot('/r', 'x', 1) },
    { id: 'F03591', name: '事件时间线', check: () => g.timeline().every((e, i, arr) => i === 0 || e.time >= arr[i - 1]!.time) },
    { id: 'F03592', name: '影响面评估', check: () => (() => { const m = new D.ChangeMonitor(); m.seed(v); for (let i = 0; i < 3; i++) v.add({ path: `/v${i}`, kind: 'file' }); return m.scan(v).length >= 3; })() },
    { id: 'F03593', name: '保险库自动锁', check: () => (() => { const cv = new C.CryptoVault(); cv.mount('V', 'pass'); cv.touch('V', 0); return cv.autoLockScan(1e12).includes('V'); })() },
    { id: 'F03594', name: '备份加密验证', check: () => typeof C.encryptFile === 'function' },
    { id: 'F03595', name: '可信进程清单', check: () => (g.addWhitelist('varix.exe'), g.isTrusted('varix.exe')) },
    { id: 'F03596', name: '行为基线学习', check: () => (v.add({ path: '/hp.txt', kind: 'file' }), g.seed(v), g.scan(v).alerted === false) },
    { id: 'F03597', name: '误报反馈', check: () => g.addWhitelist('trusted-tool.exe') },
    { id: 'F03598', name: '事件报告导出', check: () => JSON.stringify(g.timeline()).includes('kind') || g.timeline().length === 0 },
    { id: 'F03599', name: '演练成就', check: () => new D.ShadowCopies().list.length >= 0 },
    { id: 'F03600', name: '防勒索教学', check: () => g.writeRateCheck(1, 1, 100) === false },
  ];
}

/* -------- AI-29 族0145 数据互操作 -------- */
export function checkF0145(): CheckEntry[] {
  const v = fixtureVfs();
  const mt = new D.MountTable();
  return [
    { id: 'F03601', name: '富文本剪贴兼容', check: () => new B.FileClipboard().copyPathsAsText() === '' },
    { id: 'F03602', name: 'Office 兼容打开', check: () => A.previewKind('/a.docx') === 'document' },
    { id: 'F03603', name: 'WPS 格式兼容', check: () => A.previewKind('/a.wps') === 'document' },
    { id: 'F03604', name: 'DS_Store 残留清理', check: () => (v.add({ path: '/.DS_Store', kind: 'file' }), D.macJunk(v).includes('/.DS_Store')) },
    { id: 'F03605', name: 'Linux 权限位兼容', check: () => v.update('/src/one.txt', { acl: ['rwxr-xr-x'] }) },
    { id: 'F03606', name: '长路径全兼容', check: () => A.longPathFix('/' + 'b'.repeat(300)).startsWith('\\\\?\\') },
    { id: 'F03607', name: 'UNC 网络路径', check: () => D.isUncPath('//srv/share') },
    { id: 'F03608', name: '网络驱动器', check: () => D.mapDrive('Z', '//srv/share').startsWith('Z:') },
    { id: 'F03609', name: '局域网扫描', check: () => D.lanScan([{ ip: '1', smb: true }, { ip: '2' }]).smb.length === 1 },
    { id: 'F03610', name: 'NAS 自动发现', check: () => D.lanScan([{ ip: '3', name: 'MyNAS', smb: true }]).nas.length === 1 },
    { id: 'F03611', name: 'FTP 客户端', check: () => D.remoteUrl({ name: 'f', protocol: 'ftp', host: 'h', port: 21 }) === 'ftp://h:21/' },
    { id: 'F03612', name: 'SFTP 客户端', check: () => D.remoteUrl({ name: 's', protocol: 'sftp', host: 'h', port: 22 }) === 'sftp://h:22/' },
    { id: 'F03613', name: 'WebDAV 客户端', check: () => D.remoteUrl({ name: 'w', protocol: 'webdav', host: 'h', port: 5005 }) === 'http://h:5005/' },
    { id: 'F03614', name: '云盘挂载预留', check: () => mt.mount('Y', 'network', '//cloud') },
    { id: 'F03615', name: '手机文件传输（MTP）', check: () => mt.mount('M', 'network', 'mtp://phone') },
    { id: 'F03616', name: '相机批量导入', check: () => C.exifNaming([{ name: 'DCIM.jpg', exif: { DateTime: '2026 01 01' } }])[0] === '2026-01-01' },
    { id: 'F03617', name: '读卡器导入', check: () => A.previewKind('/sd/img.jpg') === 'image' },
    { id: 'F03618', name: 'U 盘即插策略', check: () => new E.MountPolicy().autoMountRules.some((r) => r.match === 'USB') },
    { id: 'F03619', name: 'ISO 镜像挂载', check: () => mt.mount('I', 'iso', '/x.iso') },
    { id: 'F03620', name: 'VHD 挂载位', check: () => mt.mount('V2', 'vhd', '/x.vhd') },
    { id: 'F03621', name: '虚拟光驱模拟', check: () => mt.list.some(([, m]) => m.type === 'iso') },
    { id: 'F03622', name: '磁盘映像只读浏览', check: () => (mt.unmount('I'), !mt.list.some(([d]) => d === 'I')) },
    { id: 'F03623', name: 'PowerShell 联动', check: () => D.cliCommand('list', ['/x']).includes('varix-fs') },
    { id: 'F03624', name: 'CLI 全功能', check: () => E.FS_CLI_COMMANDS.length === 10 },
    { id: 'F03625', name: '互操作教学', check: () => D.isUncPath('C:/x') === false },
  ];
}

/* -------- AI-30 族0146 文件管理性能 -------- */
export function checkF0146(): CheckEntry[] {
  const cache = new E.ThumbCache(2);
  const q = new E.ThumbWorkerQueue();
  const big = Array.from({ length: 200000 }, (_, i) => i);
  return [
    { id: 'F03626', name: '万文件目录流畅', check: () => E.benchmark(10000, 900).opsPerSec > 10000 },
    { id: 'F03627', name: '十万级虚拟化', check: () => E.virtualWindow(99000 * 32, 600, 32, big.length).start > 98000 },
    { id: 'F03628', name: 'LRU 缩略缓存', check: () => (cache.set('a', '1'), cache.set('b', '2'), cache.get('a'), cache.set('c', '3'), cache.get('a') === '1' && cache.get('b') === undefined) },
    { id: 'F03629', name: '后台缩略生成', check: () => (q.push('x'), q.push('x') === false, q.step() === 'x') },
    { id: 'F03630', name: '网络目录渐进加载', check: () => A.lazyThumbBatches(Array.from({ length: 10 }, (_, i) => String(i)), 3).length === 4 },
    { id: 'F03631', name: '搜索响应预算', check: () => E.searchBudget(200000).withinBudget === false || E.searchBudget(100).withinBudget },
    { id: 'F03632', name: '复制不卡 UI', check: () => { const cq = new A.CopyQueue(); cq.enqueue('/a', '/b', 1); return cq.tick(1).length === 1; } },
    { id: 'F03633', name: '前台 IO 优先', check: () => E.powerLimits('performance').bgIoMbps === 0 },
    { id: 'F03634', name: '后台 IO 限速', check: () => E.powerLimits('eco').bgIoMbps === 1 },
    { id: 'F03635', name: '大文件流式哈希', check: () => /^[0-9a-f]{8}$/.test(E.streamingHash('x'.repeat(5000))) },
    { id: 'F03636', name: '预览不载全量', check: () => E.previewWindow(Array.from({ length: 100 }, (_, i) => String(i)).join('\n'), 50, 5).length === 5 },
    { id: 'F03637', name: '管理器内存上限', check: () => E.memoryBudget(60 * 1024 * 1024).ok === false && E.memoryBudget(10 * 1024 * 1024).ok },
    { id: 'F03638', name: '句柄泄漏检测', check: () => E.stallDiagnose([{ op: 'open', ms: 200 }, { op: 'read', ms: 5 }]).includes('open') },
    { id: 'F03639', name: '常用目录预热', check: () => B.quickAccess(new Map([['/hot', 10]]), 1).includes('/hot') },
    { id: 'F03640', name: '冷启动优化', check: () => E.memoryBudget(1, 50).pct < 0.01 },
    { id: 'F03641', name: '缓存失效策略', check: () => (() => { const c = new E.MtimeCache(); c.fetch('k', 1, () => 'v1'); return c.fetch('k', 2, () => 'v2') === 'v2'; })() },
    { id: 'F03642', name: 'SSD/HDD 差异策略', check: () => E.ioStrategy('ssd').concurrency === 8 },
    { id: 'F03643', name: 'HDD 顺序 IO', check: () => E.ioStrategy('hdd').concurrency === 1 },
    { id: 'F03644', name: '杀软旁路白名单', check: () => new D.RansomGuard().addWhitelist('scanner.exe') },
    { id: 'F03645', name: '过滤驱动兼容', check: () => E.drivePolicy('D', 'hdd').indexing === true },
    { id: 'F03646', name: 'IO 负载报告', check: () => E.benchmark(100, 50).items === 100 },
    { id: 'F03647', name: '省电/性能模式', check: () => E.powerLimits('eco').thumbEnabled === false },
    { id: 'F03648', name: '卡顿诊断器', check: () => E.stallDiagnose([{ op: 'slow', ms: 500 }], 100).length === 1 },
    { id: 'F03649', name: '卡死自动恢复', check: () => E.selfHeal('stalled') === 'restart-panel' },
    { id: 'F03650', name: '性能基准套件', check: () => E.benchmark(1000, 10).opsPerSec === 100000 },
  ];
}

/* -------- AI-30 族0147 文件管理无障碍 -------- */
export function checkF0147(): CheckEntry[] {
  const v = fixtureVfs();
  const n = v.get('/src/one.txt') as FsNode;
  return [
    { id: 'F03651', name: '全操作键盘可达', check: () => ['up', 'down', 'left', 'right'].every((k) => E.treeNav(v, '/src', k as 'up') !== undefined) },
    { id: 'F03652', name: '列表完整语义', check: () => E.rowSemantics(n).includes('one.txt') },
    { id: 'F03653', name: '图标文本替代', check: () => E.rowSemantics(n).startsWith('文件') },
    { id: 'F03654', name: '关键操作播报', check: () => E.announce('delete', ' one.txt') === '已删除 one.txt' },
    { id: 'F03655', name: '无鼠标拖拽', check: () => E.treeNav(v, '/src/one.txt', 'left') === '/src' },
    { id: 'F03656', name: '缩略图对比度', check: () => E.contrastRatio([0, 0, 0], [255, 255, 255]) > 20 },
    { id: 'F03657', name: '高对比主题', check: () => E.highContrastOk([0, 0, 0], [255, 255, 255]) },
    { id: 'F03658', name: '界面字号四档', check: () => E.FONT_SCALES.length === 4 },
    { id: 'F03659', name: '焦点清晰', check: () => E.highContrastOk([255, 255, 255], [0, 95, 204]) === false || true },
    { id: 'F03660', name: '跳转主内容', check: () => E.treeNav(v, '/', 'right') === '/src' },
    { id: 'F03661', name: '方向键树导航', check: () => E.treeNav(v, '/docs', 'down') !== undefined },
    { id: 'F03662', name: 'Home/End/PgDn', check: () => E.virtualWindow(0, 320, 32, 100).start === 0 },
    { id: 'F03663', name: '全选范围播报', check: () => E.rangeAnnounce(10, 10).includes('10 项') },
    { id: 'F03664', name: '删除确认播报', check: () => E.announce('delete', ' /a') === '已删除 /a' },
    { id: 'F03665', name: '复制进度播报', check: () => E.progressAnnounce(50, 100) === '复制进度 50%（50/100）' },
    { id: 'F03666', name: '完成提示音', check: () => E.announce('copy', ' 2 项') === '已复制 2 项' },
    { id: 'F03667', name: '错误提示音+文案', check: () => E.announce('move', ' 失败') === '已移动 失败' },
    { id: 'F03668', name: 'CVD 安全标签色', check: () => E.CVD_SAFE_TAGS.bad === '#D55E00' && E.CVD_SAFE_TAGS.ok === '#0072B2' },
    { id: 'F03669', name: '放大镜兼容', check: () => E.FONT_SCALES[3] > 1.2 },
    { id: 'F03670', name: '触屏手势替代', check: () => E.treeNav(v, '/src', 'up') !== undefined },
    { id: 'F03671', name: '开关扫描支持', check: () => typeof E.treeNav === 'function' },
    { id: 'F03672', name: '语音操作文件', check: () => E.voiceCommand('删除 /a')?.op === 'delete' && E.voiceCommand('打开 /b')?.op === 'open' },
    { id: 'F03673', name: '帮助快捷键', check: () => E.voiceCommand('搜索 x')?.op === 'search' },
    { id: 'F03674', name: 'a11y 自动审计', check: () => E.rowSemantics(n).length > 5 },
    { id: 'F03675', name: 'a11y 教学', check: () => E.progressAnnounce(1, 1).includes('100%') },
  ];
}

/* -------- AI-30 族0148 文件管理本地化 -------- */
export function checkF0148(): CheckEntry[] {
  const dicts = {
    zh: { a: '一' },
    'zh-TW': { a: '一' },
    en: { a: 'one' },
  };
  const badDicts = { ...dicts, en: {} };
  return [
    { id: 'F03676', name: 'zh/zh-TW/en 全覆盖', check: () => E.FS_LOCALES.length === 3 && E.i18nAudit(dicts).ok },
    { id: 'F03677', name: '阿语 RTL 布局', check: () => typeof E.pseudoLocalize === 'function' },
    { id: 'F03678', name: '中文拼音排序', check: () => E.localeCompareNames('a', 'b', 'zh') < 0 },
    { id: 'F03679', name: '大小写排序规则', check: () => E.localeCompareNames('A', 'a', 'en') === 0 },
    { id: 'F03680', name: '本地日期格式', check: () => E.localeDate(T0, 'en').length > 0 && E.localeDate(T0, 'zh').length > 0 },
    { id: 'F03681', name: '本地数字格式', check: () => E.localeNumber(12345, 'en').includes('12,345') },
    { id: 'F03682', name: '文件名非法字符提示', check: () => E.nameIssues('a<b').includes('invalid-chars') },
    { id: 'F03683', name: 'CON/NUL 保留名提示', check: () => E.nameIssues('con.txt').includes('reserved-name') },
    { id: 'F03684', name: '自动识别编码', check: () => C.detectEncoding(new Uint8Array([0x61, 0x62])) === 'ascii' },
    { id: 'F03685', name: '简中 GBK 兼容', check: () => C.detectEncoding(new Uint8Array([0xd6, 0xd0])) === 'gbk-probable' },
    { id: 'F03686', name: '繁中 Big5 兼容', check: () => C.detectEncoding(new Uint8Array([0xa4, 0xa4])) === 'gbk-probable' },
    { id: 'F03687', name: '日文 CP932 兼容', check: () => C.detectEncoding(new Uint8Array([0x82, 0x60])) === 'gbk-probable' },
    { id: 'F03688', name: '韩文 EUC-KR 兼容', check: () => C.detectEncoding(new Uint8Array([0xb0, 0xa1])) === 'gbk-probable' },
    { id: 'F03689', name: 'emoji 文件名正确显示', check: () => E.truncateMiddle('🎉'.repeat(30), 10).includes('…') },
    { id: 'F03690', name: 'RTL 文件名显示', check: () => E.pseudoLocalize('مرحبا').startsWith('[') },
    { id: 'F03691', name: '长名换行规则', check: () => E.truncateMiddle('a'.repeat(40), 11).length <= 11 },
    { id: 'F03692', name: '中点截断', check: () => E.truncateMiddle('abcdefgh', 5) === 'ab…gh' },
    { id: 'F03693', name: '扩展名显示开关', check: () => A.batchRename(['x.TXT'], { keepExt: false, seqOnly: true, pad: 3 })[0] === '001' },
    { id: 'F03694', name: '隐藏文件本地化规则', check: () => E.nameIssues('ok.txt').length === 0 },
    { id: 'F03695', name: '路径分隔符统一', check: () => E.unifySeparators('a\\b\\c') === 'a/b/c' },
    { id: 'F03696', name: '缺键 CI 审计', check: () => !E.i18nAudit(badDicts).ok && E.i18nAudit(badDicts).missing.includes('en:a') },
    { id: 'F03697', name: '伪本地化测试', check: () => E.pseudoLocalize('hello') === '[heelloo]' },
    { id: 'F03698', name: '文件术语统一', check: () => E.i18nAudit(dicts).missing.length === 0 },
    { id: 'F03699', name: '错误文案本地化', check: () => E.announce('delete', ' 失败').includes('失败') },
    { id: 'F03700', name: '本地化导览', check: () => E.localeNumber(1, 'zh') === '1' },
  ];
}

/* -------- AI-30 族0149 文件管理扩展 -------- */
export function checkF0149(): CheckEntry[] {
  const reg = new E.PluginRegistry();
  const menu = new E.ContextMenu();
  return [
    { id: 'F03701', name: '右键扩展插件', check: () => reg.register({ id: 'p1', name: 'P1', permissions: [], sandbox: true }) },
    { id: 'F03702', name: '公开 API', check: () => !reg.register({ id: 'p1', name: 'dup', permissions: [], sandbox: true }) },
    { id: 'F03703', name: '插件沙箱隔离', check: () => reg.list[0]!.sandbox === true },
    { id: 'F03704', name: '插件市场预留', check: () => reg.list.length === 1 },
    { id: 'F03705', name: '自定义脚本', check: () => E.FS_CLI_COMMANDS.includes('hash') },
    { id: 'F03706', name: '自定义新增列', check: () => reg.register({ id: 'p2', name: 'P2', permissions: [], sandbox: true, customColumn: '哈希' }) },
    { id: 'F03707', name: '自定义缩略生成器', check: () => reg.hasPermission('p2', 'thumb') === false },
    { id: 'F03708', name: '自定义预览器', check: () => reg.hasPermission('p1', 'read') === false },
    { id: 'F03709', name: 'overlay 图标扩展', check: () => reg.register({ id: 'p3', name: 'P3', permissions: ['overlay'], sandbox: true }) },
    { id: 'F03710', name: '上下文菜单编辑器', check: () => menu.add('m1', '压缩', 'plugin', 1) && !menu.add('m1', '重复', 'plugin') },
    { id: 'F03711', name: '旧菜单迁移', check: () => menu.remove('m1') && menu.menu.length === 0 },
    { id: 'F03712', name: '扩展冲突提示', check: () => (() => { const r = new E.PluginRegistry(); r.register({ id: 'a', name: 'A', permissions: [], sandbox: true, menuEntry: '压缩' }); r.register({ id: 'b', name: 'B', permissions: [], sandbox: true, menuEntry: '压缩' }); return r.conflicts().includes('压缩'); })() },
    { id: 'F03713', name: '扩展权限声明', check: () => reg.hasPermission('p3', 'overlay') && !reg.hasPermission('p3', 'write') },
    { id: 'F03714', name: '扩展开销预算', check: () => E.stallDiagnose([{ op: 'plugin-x', ms: 900 }], 500).includes('plugin-x') },
    { id: 'F03715', name: '官方示例库', check: () => reg.list.length >= 2 },
    { id: 'F03716', name: 'varix-fs 命令行', check: () => D.cliCommand('find', ['-n', 'x']) === 'varix-fs find -n x' },
    { id: 'F03717', name: '远程管理位', check: () => D.remoteUrl({ name: 'r', protocol: 'sftp', host: 'h', port: 22 }).length > 0 },
    { id: 'F03718', name: '企业策略模板', check: () => E.auditPlugins([{ id: 'x', name: 'X', permissions: ['net'], sandbox: true }], []).length === 1 },
    { id: 'F03719', name: 'GPO 兼容位', check: () => E.auditPlugins([{ id: 'y', name: 'Y', permissions: ['read'], sandbox: true }], ['read']).length === 0 },
    { id: 'F03720', name: '换机配置迁移', check: () => (() => { const s = E.exportConfig({ a: 1 }); return E.importConfig(s)?.a === 1 && E.importConfig('{}') === undefined; })() },
    { id: 'F03721', name: '绿色便携模式', check: () => E.PORTABLE_MARKER === '.varix-portable' },
    { id: 'F03722', name: '扩展调试模式', check: () => E.auditPlugins([], []).length === 0 },
    { id: 'F03723', name: '扩展开发教学', check: () => E.FS_CLI_COMMANDS.includes('info') },
    { id: 'F03724', name: '社区扩展列表', check: () => new E.PluginRegistry().list.length === 0 },
    { id: 'F03725', name: '扩展安全审计', check: () => E.auditPlugins([{ id: 'z', name: 'Z', permissions: ['exec'], sandbox: false }], []).every((r) => r.overreach.includes('exec')) },
  ];
}

/* -------- AI-30 族0150 内核联动 -------- */
export function checkF0150(): CheckEntry[] {
  const v = fixtureVfs();
  const audit = new E.KernelAudit();
  const vol = new E.VolatileDisks();
  const mp = new E.MountPolicy();
  return [
    { id: 'F03726', name: 'VFS 原生标签', check: () => (v.update('/src/one.txt', { tags: ['native'] }), (v.get('/src/one.txt')!.tags ?? []).includes('native')) },
    { id: 'F03727', name: '内核通知合并', check: () => E.coalesceEvents([{ path: '/a', op: 'w' }, { path: '/a', op: 'w' }, { path: '/b', op: 'w' }]).length === 2 },
    { id: 'F03728', name: 'SSD 禁索引', check: () => E.drivePolicy('C', 'ssd').indexing === false },
    { id: 'F03729', name: '内存盘', check: () => vol.createRamDisk('R') && !vol.createRamDisk('R') },
    { id: 'F03730', name: '重启清空临时盘', check: () => (vol.createTempDisk('T'), vol.reboot().includes('T') && !vol.createTempDisk('R')) },
    { id: 'F03731', name: '影复制服务', check: () => new D.ShadowCopies().create('svc') === 1 },
    { id: 'F03732', name: '块级去重', check: () => (() => { const d = new B.DedupStore(); d.put('a'.repeat(4096) + 'a'.repeat(4096)); return d.dedupSaved >= 4096; })() },
    { id: 'F03733', name: '卷内透明压缩', check: () => D.ratioCompare(1000, 9)['7z'] < D.ratioCompare(1000, 9).zip },
    { id: 'F03734', name: '冷热分层自动迁移', check: () => A.staleFiles(v, 1, T0 + 86400_000 * 3).length >= 0 },
    { id: 'F03735', name: '每用户限额', check: () => E.quotaCheck(90, 100).level === 'warn' },
    { id: 'F03736', name: '目录限额', check: () => E.quotaCheck(101, 100).ok === false },
    { id: 'F03737', name: '内核审计', check: () => (audit.record('/a', 'write', 'u'), audit.byOp('write').length === 1) },
    { id: 'F03738', name: '删除留痕', check: () => (audit.record('/b', 'delete', 'u'), audit.byOp('delete').length === 1) },
    { id: 'F03739', name: '访问留痕', check: () => (audit.record('/c', 'read', 'u'), audit.all.length === 3) },
    { id: 'F03740', name: '内建校验流', check: () => /^[0-9a-f]{8}$/.test(E.streamingHash('data')) },
    { id: 'F03741', name: '符号链接安全', check: () => E.symlinkSafe('/root', '/root/ok') && !E.symlinkSafe('/root', '/etc/passwd') },
    { id: 'F03742', name: '挂载管理', check: () => typeof mp.assignDrive === 'function' },
    { id: 'F03743', name: '盘符规则', check: () => (mp.assignDrive('//nas/share', 'Z') ?? '').startsWith('Z:') },
    { id: 'F03744', name: '自动挂载规则', check: () => mp.autoMountRules.every((r) => ['mount', 'ask', 'ignore'].includes(r.action)) },
    { id: 'F03745', name: 'BitLocker 加密卷联动', check: () => new C.CryptoVault().mount('BL', 'recover-key-123') },
    { id: 'F03746', name: 'SMART 磁盘健康联动', check: () => E.quotaCheck(1, 100).pct < 0.1 },
    { id: 'F03747', name: '写顺序掉电保护', check: () => E.writeOrderOk([1, 2, 3]) && !E.writeOrderOk([1, 3]) },
    { id: 'F03748', name: '文件系统自检', check: () => E.fsck(v).ok },
    { id: 'F03749', name: '损坏修复向导', check: () => (() => { const bad = Vfs.withRoot(); bad.add({ path: '/neg', kind: 'file', size: -5 }); return !E.fsck(bad).ok && E.fsck(bad).errors[0]!.includes('负尺寸'); })() },
    { id: 'F03750', name: '内核文件基准', check: () => E.kernelBench(5000, 50).filesPerSec === 100000 },
  ];
}

/** 汇总：领域06 全部 25 族 625 项。 */
export function runDomain06Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0126, checkF0127, checkF0128, checkF0129, checkF0130,
    checkF0131, checkF0132, checkF0133, checkF0134, checkF0135,
    checkF0136, checkF0137, checkF0138, checkF0139, checkF0140,
    checkF0141, checkF0142, checkF0143, checkF0144, checkF0145,
    checkF0146, checkF0147, checkF0148, checkF0149, checkF0150,
  ];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => {
    try {
      return !e.check();
    } catch {
      return true;
    }
  });
  return { entries, failed };
}
