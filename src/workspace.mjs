import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const packageRoot = fileURLToPath(new URL('../', import.meta.url));
export function assertNode() {
  const [major, minor, patch] = process.versions.node.split('.').map(Number);
  if (major < 24 || (major === 24 && (minor < 22 || (minor === 22 && patch < 1)))) throw new Error('Node 24.22.1 이상이 필요합니다. 최신 Node.js LTS를 설치하세요.');
}
export function defaultWorkspace() {
  if (process.platform !== 'win32') return path.join(os.homedir(), 'Documents', 'autosw-workspace');
  const desktop = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
    '[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); [Console]::Write([Environment]::GetFolderPath("DesktopDirectory"))'
  ], { encoding: 'utf8', windowsHide: true, timeout: 15000 });
  if (desktop.status !== 0 || !path.isAbsolute(desktop.stdout || '')) {
    throw new Error('Windows 바탕화면 위치를 확인하지 못했습니다. --workspace <개인폴더>로 위치를 지정하세요.');
  }
  return path.join(desktop.stdout, 'autosw-workspace');
}
export function inside(root, candidate) {
  const rel = path.relative(root, candidate);
  return rel === '' || (!rel.startsWith('..' + path.sep) && rel !== '..' && !path.isAbsolute(rel));
}
export function workspaceRoot(requested) {
  const root = path.resolve(requested || process.env.AUTOSW_WORKSPACE || defaultWorkspace());
  if (!fs.existsSync(root)) throw new Error('작업 폴더가 없습니다. init --workspace <폴더>로 먼저 준비하세요.');
  const real = fs.realpathSync(root);
  if (!fs.statSync(real).isDirectory()) throw new Error('작업 위치는 폴더여야 합니다.');
  if (inside(packageRoot, real) || inside(real, packageRoot)) throw new Error('설치 폴더와 분리된 개인 작업 폴더를 사용하세요.');
  return real;
}
export function safePath(root, input, { output = false, extensions, internal = false } = {}) {
  if (typeof input !== 'string' || !input || /[\x00-\x1f]/.test(input)) throw new Error('올바른 파일 경로가 필요합니다.');
  const resolved = path.resolve(root, input);
  if (!inside(root, resolved) || resolved === root) throw new Error('작업 폴더 밖에는 접근할 수 없습니다.');
  function allowed(candidate, checkExtension = true) {
    const rel = path.relative(root, candidate);
    if (rel.includes(':') || /[. ](?:[\\/]|$)/.test(rel)) throw new Error('지원하지 않는 경로 표기입니다.');
    const parts = rel.split(/[\\/]/);
    if (parts.some(p => /^(?:\.env(?:\..*)?|\.git|node_modules|\.local)$/i.test(p)) || (!internal && parts.some(p => p.startsWith('.')))) throw new Error('설정·비밀정보·내부 파일에는 접근할 수 없습니다.');
    if (checkExtension && extensions && !extensions.includes(path.extname(candidate).toLowerCase())) throw new Error('지원하지 않는 파일 확장자입니다.');
  }
  allowed(resolved);
  // Junctions/symlinks may exist at any level, including a file itself.
  const check = output && !fs.existsSync(resolved) ? path.dirname(resolved) : resolved;
  const real = fs.realpathSync(check);
  if (!inside(root, real)) throw new Error('연결된 경로가 작업 폴더 밖을 가리킵니다.');
  allowed(real, !(output && !fs.existsSync(resolved)));
  if (output && !fs.statSync(path.dirname(resolved)).isDirectory()) throw new Error('출력 폴더가 없습니다.');
  if (!output && !fs.statSync(real).isFile()) throw new Error('파일을 지정하세요.');
  return output ? path.join(fs.realpathSync(path.dirname(resolved)), path.basename(resolved)) : real;
}
export function readText(root, input, extensions = ['.md', '.json']) {
  const file = safePath(root, input, { extensions });
  if (fs.statSync(file).size > 4 * 1024 * 1024) throw new Error('텍스트 입력은 4MB 이하만 지원합니다.');
  return new TextDecoder('utf-8', { fatal: true }).decode(fs.readFileSync(file)).replace(/^\uFEFF/, '');
}
export function initialize(requested) {
  const root = path.resolve(requested || defaultWorkspace());
  if (inside(packageRoot, root) || inside(root, packageRoot)) throw new Error('설치 폴더와 분리된 개인 작업 폴더를 지정하세요.');
  fs.mkdirSync(root, { recursive: true });
  const real = workspaceRoot(root);
  for (const name of ['drafts', 'templates', 'outputs', 'previews', '.autosw']) {
    const folder = path.join(real, name);
    fs.mkdirSync(folder, { recursive: true });
    if (!inside(real, fs.realpathSync(folder))) throw new Error('작업 하위 폴더의 연결 경로를 확인하세요.');
  }
  const config = path.join(real, 'autosw.config.json');
  if (!fs.existsSync(config)) fs.writeFileSync(config, JSON.stringify({ schemaVersion: 1, institution: '', writingPreferences: '', templateDirectory: 'templates', outputDirectory: 'outputs' }, null, 2) + '\n', { flag: 'wx' });
  return { status: 'ready', workspace: real, configPath: config, note: '기관명과 작성 선호만 설정하세요. 실제 양식은 templates에 직접 넣어 주세요.' };
}
