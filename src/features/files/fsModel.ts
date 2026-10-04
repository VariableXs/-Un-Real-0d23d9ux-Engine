// AURORA-10000: AI-26~AI-30 批次（领域06 文件与数据能力）公共模型，勿删。
// 虚拟文件系统模型：领域06 全部 25 个族（F03126~F03750）的逻辑核都基于本模型运行，
// 与真实磁盘解耦，便于纯函数测试与后续接入 src-tauri IPC 层。

export type NodeKind = 'file' | 'dir';

export interface FsNode {
  path: string; // 绝对路径，'/' 为根
  kind: NodeKind;
  size: number; // 字节；目录为聚合值
  mtime: number;
  ctime: number;
  atime: number;
  readonly?: boolean;
  hidden?: boolean;
  system?: boolean;
  owner?: string;
  acl?: string[];
  content?: string; // 文本内容（测试用）
  tags?: string[];
  comment?: string;
  rating?: number;
  fields?: Record<string, string>;
  versionNote?: string;
  project?: string;
  versions?: { time: number; size: number; note?: string }[];
  hash?: string;
  exif?: Record<string, string>;
  lockedBy?: string; // 占用检测
}

export function parentOf(path: string): string {
  if (path === '/' || !path.includes('/')) return '/';
  const p = path.slice(0, path.lastIndexOf('/'));
  return p === '' ? '/' : p;
}

export function baseName(path: string): string {
  return path === '/' ? '/' : path.slice(path.lastIndexOf('/') + 1);
}

export function extOf(name: string): string {
  const b = baseName(name);
  const i = b.lastIndexOf('.');
  return i <= 0 ? '' : b.slice(i + 1).toLowerCase();
}

export function joinPath(dir: string, name: string): string {
  return dir === '/' ? `/${name}` : `${dir}/${name}`;
}

/** 虚拟文件系统：登记类接口均自带去重（项目约定）。 */
export class Vfs {
  private nodes = new Map<string, FsNode>();

  static withRoot(): Vfs {
    const v = new Vfs();
    v.nodes.set('/', { path: '/', kind: 'dir', size: 0, mtime: 0, ctime: 0, atime: 0 });
    return v;
  }

  has(path: string): boolean {
    return this.nodes.has(path);
  }

  get(path: string): FsNode | undefined {
    return this.nodes.get(path);
  }

  /** 新增节点；路径重复时返回 false（去重）。 */
  add(node: Partial<FsNode> & { path: string; kind: NodeKind }): boolean {
    if (this.nodes.has(node.path)) return false;
    const parent = parentOf(node.path);
    if (node.path !== '/' && !this.nodes.has(parent)) return false;
    const now = 1000;
    this.nodes.set(node.path, {
      size: 0,
      mtime: now,
      ctime: now,
      atime: now,
      ...node,
    });
    return true;
  }

  /** 删除节点（目录级联）。返回删除的节点数量。 */
  remove(path: string): number {
    if (!this.nodes.has(path)) return 0;
    let n = 0;
    for (const p of [...this.nodes.keys()]) {
      if (p === path || p.startsWith(path === '/' ? '/' : path + '/')) {
        this.nodes.delete(p);
        n++;
      }
    }
    return n;
  }

  children(dir: string): FsNode[] {
    const out: FsNode[] = [];
    for (const n of this.nodes.values()) {
      if (n.path !== dir && parentOf(n.path) === dir) out.push(n);
    }
    return out;
  }

  all(): FsNode[] {
    return [...this.nodes.values()];
  }

  update(path: string, patch: Partial<FsNode>): boolean {
    const n = this.nodes.get(path);
    if (!n) return false;
    this.nodes.set(path, { ...n, ...patch });
    return true;
  }

  /** 快速构造一棵演示树。 */
  static demo(): Vfs {
    const v = Vfs.withRoot();
    v.add({ path: '/docs', kind: 'dir' });
    v.add({ path: '/pics', kind: 'dir' });
    v.add({ path: '/docs/a.txt', kind: 'file', size: 120, content: 'hello aurora' });
    v.add({ path: '/docs/b.md', kind: 'file', size: 340, content: '# t' });
    v.add({ path: '/pics/c.jpg', kind: 'file', size: 204800, exif: { GPS: '31.2,121.4' } });
    return v;
  }
}
