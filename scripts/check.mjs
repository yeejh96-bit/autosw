import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { packageRoot, assertNode } from '../src/workspace.mjs';

assertNode();
const mode = process.argv[2];
if (mode && !['--lint', '--test', '--docs'].includes(mode)) throw new Error('잘못된 검증 옵션');
const files = [];
function walk(dir) {
  for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
    if (['node_modules', '.git', '.local', '.autosw', 'workspace'].includes(item.name)) continue;
    const file = path.join(dir, item.name);
    if (item.isDirectory()) walk(file); else files.push(file);
  }
}
walk(packageRoot);
function run(file, args = []) {
  const result = spawnSync(process.execPath, [file, ...args], { cwd: packageRoot, stdio: 'inherit', timeout: 300000, env: { ...process.env, PATH: path.dirname(process.execPath) + path.delimiter + process.env.PATH } });
  if (result.error || result.status !== 0) throw new Error(`검증 실패: ${path.relative(packageRoot, file)} ${result.error?.message || result.status}`);
}
function docs() {
  for (const file of files.filter(f => f.endsWith('.md') && !f.includes(path.sep + 'vendor' + path.sep))) {
    const content = fs.readFileSync(file, 'utf8');
    if (/C:[\\/]Users[\\/]s2|\.\.\/\.\.\/\.\.\/\.\.\//i.test(content)) throw new Error(`개인 절대경로 또는 상위 프로젝트 의존: ${file}`);
    for (const match of content.matchAll(/\[[^\]]*\]\(([^)]+)\)/g)) {
      const target = match[1].split('#')[0];
      if (!target || /^(?:https?:|mailto:)/.test(target) || /[<>]/.test(target)) continue;
      if (!fs.existsSync(path.resolve(path.dirname(file), target))) throw new Error(`끊긴 문서 링크: ${file} -> ${target}`);
    }
    if (path.basename(file) === 'SKILL.md' && (!content.startsWith('---') || !/^name: autosw-/m.test(content) || !/^description: .+/m.test(content))) throw new Error(`스킬 머리말 오류: ${file}`);
  }
  for (const name of ['README.md', 'AGENTS.md', 'LICENSE', 'NOTICE', 'plugin.json', 'mcp.json', 'skills/autosw-document/SKILL.md']) if (!fs.existsSync(path.join(packageRoot, name))) throw new Error(`필수 파일 누락: ${name}`);
  const pkg = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'));
  const plugin = JSON.parse(fs.readFileSync(path.join(packageRoot, 'plugin.json'), 'utf8'));
  if (pkg.version !== plugin.version || pkg.private !== false || pkg.publishConfig?.registry !== 'https://registry.npmjs.org/' || pkg.publishConfig?.access !== 'public') throw new Error('배포 메타데이터/대상 레지스트리 불일치');
  const mcp = JSON.parse(fs.readFileSync(path.join(packageRoot, 'mcp.json'), 'utf8')).mcpServers.autosw;
  if (mcp.command !== 'cmd.exe' || JSON.stringify(mcp.args) !== JSON.stringify(['/d', '/c', 'npx', '--yes', `${pkg.name}@${pkg.version}`, 'mcp']) || mcp.env?.AUTOSW_WORKSPACE !== '') throw new Error('플러그인 실행 버전·기본 개인 폴더 설정 불일치');
  console.log('지침·참조·배포 메타데이터 확인 통과');
}
if (!mode || mode === '--lint') {
  for (const file of files.filter(f => f.endsWith('.mjs'))) run('--check', [file]);
  for (const file of files.filter(f => f.endsWith('.json'))) JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
  for (const file of files.filter(f => /\.(ps1|psm1)$/.test(f))) if (!fs.readFileSync(file).subarray(0, 3).equals(Buffer.from([239, 187, 191]))) throw new Error(`PowerShell UTF-8 BOM 누락: ${file}`);
  console.log('문법·JSON·PowerShell 인코딩 확인 통과');
}
if (!mode || mode === '--docs') docs();
if (!mode || mode === '--test') {
  run(path.join(packageRoot, 'tests', 'setup-npm.test.mjs'));
  run(path.join(packageRoot, 'tests', 'ledger.test.mjs'));
  run(path.join(packageRoot, 'tests', 'service.test.mjs'));
  run(path.join(packageRoot, 'tests', 'mcp.test.mjs'));
  for (const name of ['test-hwpx-form.mjs', 'test-hwpx-create.mjs', 'test-hwpx-layout.mjs', 'test-hangul-preview.mjs']) run(path.join(packageRoot, 'tests', 'engine', name));
  console.log('자동 검증 통과 (실제 한글 COM·전 페이지 검토는 별도)');
}
