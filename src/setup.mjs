import fs from 'node:fs';
import path from 'node:path';
import { workspaceRoot, packageRoot, inside } from './workspace.mjs';
import { writeAtomic } from '../engine/atomic-file.mjs';
export function setup(workspace, apply = false, { launcher = 'local' } = {}) {
  const root = workspaceRoot(workspace);
  const configDir = path.join(root, '.codex'), config = path.join(configDir, 'config.toml');
  const skillParent = path.join(root, '.agents', 'skills'), skill = path.join(skillParent, 'autosw-document');
  if (!['local', 'npm'].includes(launcher)) throw new Error('지원하지 않는 실행 방식');
  const pkg = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json')));
  if (pkg.name !== 'autosw' || !/^\d+\.\d+\.\d+$/.test(pkg.version)) throw new Error('패키지 이름·버전 확인 필요');
  const entry = path.join(packageRoot, 'bin', 'autosw.mjs');
  const command = launcher === 'local' ? process.execPath : process.platform === 'win32' ? 'cmd.exe' : 'npx';
  const args = launcher === 'local' ? [entry, 'mcp', '--workspace', root] : [...(process.platform === 'win32' ? ['/d', '/c', 'npx'] : []), '--yes', `autosw@${pkg.version}`, 'mcp'];
  // JSON basic strings are also valid for these TOML string values (paths contain no control characters).
  const content = `[mcp_servers.autosw]\ncommand = ${JSON.stringify(command)}\nargs = ${JSON.stringify(args)}\nstartup_timeout_sec = ${launcher === 'npm' ? 120 : 30}\ntool_timeout_sec = 180\n` + (launcher === 'npm' ? `\n[mcp_servers.autosw.env]\nAUTOSW_WORKSPACE = ${JSON.stringify(root)}\n` : '');
  const plan = { status: 'setup-preview', workspace: root, configPath: config, config: content, skillPath: skill, note: '지정 작업 폴더에만 설치합니다. 기존 설정은 덮어쓰지 않습니다. 설치 뒤 해당 폴더를 Codex 프로젝트로 열고 신뢰 설정을 확인하세요. 웹 ChatGPT 연결은 별도입니다.' };
  if (!apply) return plan;
  for (const target of [config, skill]) if (fs.existsSync(target)) throw new Error(`기존 설치를 보존했습니다. 다른 빈 작업 폴더를 사용하거나 기존 설정을 직접 검토하세요: ${target}`);
  for (const dir of [configDir, skillParent]) {
    let ancestor = dir;
    while (!fs.existsSync(ancestor)) ancestor = path.dirname(ancestor);
    if (!inside(root, fs.realpathSync(ancestor))) throw new Error('설치 경로가 작업 폴더 밖을 가리킵니다.');
    fs.mkdirSync(dir, { recursive: true });
  }
  const staging = fs.mkdtempSync(path.join(skillParent, '.autosw-setup-'));
  const stagedSkill = path.join(staging, 'skill');
  try {
    fs.cpSync(path.join(packageRoot, 'skills', 'autosw-document'), stagedSkill, { recursive: true, errorOnExist: true, force: false });
    fs.renameSync(stagedSkill, skill);
    writeAtomic(config, content, { mode: 'new' });
  } catch (e) {
    throw new Error(`설치가 일부 완료됐을 수 있습니다. ${skill} 및 ${config}를 확인하세요. ${e.message}`);
  } finally {
    function cleanOwned(dir) {
      for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
        const file = path.join(dir, item.name);
        if (item.isDirectory() && !item.isSymbolicLink()) { cleanOwned(file); fs.rmdirSync(file); }
        else fs.unlinkSync(file);
      }
    }
    cleanOwned(staging); fs.rmdirSync(staging);
  }
  return { ...plan, status: 'setup-written', note: '작업 폴더의 설정·스킬을 저장했습니다. 앱에서 프로젝트 신뢰와 MCP 연결 여부를 확인하세요.' };
}
