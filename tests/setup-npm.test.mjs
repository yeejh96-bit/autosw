import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { npmSetup } from '../src/setup-npm.mjs';
import { packageRoot } from '../src/workspace.mjs';
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'autosw-npm-setup-'));
const work = path.join(root, '개인 # 작업 & 이름');
function clean(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, e.name);
    if (e.isDirectory() && !e.isSymbolicLink()) { clean(file); fs.rmdirSync(file); } else fs.unlinkSync(file);
  }
}
try {
  const p = npmSetup(work);
  assert.equal(p.status, 'npm-setup-preview'); assert.equal(fs.existsSync(work), false);
  const result = npmSetup(work, true);
  assert.equal(result.status, 'setup-written');
  const config = fs.readFileSync(result.configPath, 'utf8');
  assert.match(config, /@yeejh96\/autosw@0\.1\.1/); assert.ok(config.includes(JSON.stringify(work)));
  assert.ok(!config.includes(packageRoot));
  const argsLine = config.split('\n').find(l => l.startsWith('args = '));
  const args = JSON.parse(argsLine.slice(7));
  assert.deepEqual(args, ['/d', '/c', 'npx', '--yes', '@yeejh96/autosw@0.1.1', 'mcp']);
  assert.ok(!args.some(a => a.includes(work)), '사용자 경로를 셸 인자로 결합하지 않음');
  assert.ok(fs.existsSync(path.join(work, '.agents/skills/autosw-document/references/ledger.md')));
  assert.throws(() => npmSetup(work, true), /기존/); assert.equal(fs.readFileSync(result.configPath, 'utf8'), config);
  assert.equal(fs.existsSync(path.join(work, '.autosw/ledger.json')), false);
  const previewWork = path.join(root, '새 폴더');
  const cli = spawnSync(process.execPath, [path.join(packageRoot, 'bin/autosw.mjs'), 'setup', '--npm', '--workspace', previewWork], { encoding: 'utf8', windowsHide: true });
  assert.equal(cli.status, 0, cli.stderr); assert.equal(JSON.parse(cli.stdout).status, 'npm-setup-preview'); assert.equal(fs.existsSync(previewWork), false);
  const actual = spawnSync(process.execPath, [path.join(packageRoot, 'bin/autosw.mjs'), 'setup', '--npm', '--workspace', previewWork, '--apply'], { encoding: 'utf8', windowsHide: true });
  assert.equal(actual.status, 0, actual.stderr); assert.equal(JSON.parse(actual.stdout).status, 'setup-written');
  console.log('npm 연결: 미리보기 무변경·실제 CLI 설치·고정 버전·캐시 독립·경로 인자 분리·기존 설정 보존 통과');
} finally { assert.equal(path.dirname(root).toLowerCase(), fs.realpathSync(os.tmpdir()).toLowerCase()); clean(root); fs.rmdirSync(root); }
