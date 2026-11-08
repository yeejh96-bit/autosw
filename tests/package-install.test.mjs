import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
if (process.argv.length !== 3) throw new Error('npm pack으로 만든 TGZ 파일을 지정하세요.');
const archive = path.resolve(process.argv[2]);
const npm = path.join(path.dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'autosw-package-'));
const installed = path.join(root, '직원 설치'), workspace = path.join(root, '개인 업무');
function clean(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, e.name);
    if (e.isDirectory() && !e.isSymbolicLink()) { clean(file); fs.rmdirSync(file); } else fs.unlinkSync(file);
  }
}
function run(args, cwd = root) {
  const r = spawnSync(process.execPath, args, { cwd, encoding: 'utf8', windowsHide: true, timeout: 240000, env: { ...process.env, PATH: path.dirname(process.execPath) + path.delimiter + process.env.PATH } });
  assert.ifError(r.error); assert.equal(r.status, 0, r.stdout + r.stderr); return r.stdout;
}
try {
  fs.mkdirSync(installed);
  run([npm, 'install', '--prefix', installed, '--ignore-scripts', '--no-audit', '--no-fund', '--cache', path.join(root, 'npm-cache'), archive]);
  const app = path.join(installed, 'node_modules/autosw'), cli = path.join(app, 'bin/autosw.mjs');
  const preview = JSON.parse(run([cli, 'setup', '--npm', '--workspace', workspace]));
  assert.equal(preview.status, 'npm-setup-preview'); assert.equal(fs.existsSync(workspace), false);
  const actual = JSON.parse(run([cli, 'setup', '--npm', '--workspace', workspace, '--apply']));
  assert.equal(actual.status, 'setup-written');
  assert.ok(fs.readFileSync(actual.configPath, 'utf8').includes('autosw@0.1.0'));
  run([path.join(app, 'scripts/check-connection.mjs'), '--workspace', workspace]);
  run([cli, 'write-draft', '--workspace', workspace, '--output', 'drafts/test.md', '--content', '# 합성 패키지 시험\n\n설치한 패키지로 생성한 문서입니다.']);
  const plan = JSON.parse(run([cli, 'preview', '--workspace', workspace, '--kind', 'create', '--input', 'drafts/test.md', '--output', 'outputs/test.hwpx']));
  run([cli, 'apply', '--workspace', workspace, '--plan', plan.planId, '--confirm', plan.confirmHash]);
  const verified = JSON.parse(run([cli, 'verify', '--workspace', workspace, '--plan', plan.planId]));
  assert.equal(verified.independentCheck, 'passed');
  const ledger = JSON.parse(run([cli, 'ledger-query', '--workspace', workspace]));
  assert.equal(ledger.totals.sessions, 0); assert.equal(fs.existsSync(path.join(workspace, '.autosw/ledger.json')), false);
  console.log('npm 설치본: TGZ 실제 설치·자체 의존성·setup·stdio 연결·HWPX 생성·독립 대조·빈 장부 확인 통과');
} finally { assert.equal(path.dirname(root).toLowerCase(), fs.realpathSync(os.tmpdir()).toLowerCase()); clean(root); fs.rmdirSync(root); }
