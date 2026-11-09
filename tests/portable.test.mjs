// An installation copied outside the source tree must resolve only its own code/dependencies.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { packageRoot } from '../src/workspace.mjs';
const base = fs.mkdtempSync(path.join(os.tmpdir(), 'autosw-portable-'));
const installed = path.join(base, '별도 # 설치'), work = path.join(base, '개인 작업');
function clean(dir) {
  for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, item.name);
    if (item.isDirectory() && !item.isSymbolicLink()) { clean(file); fs.rmdirSync(file); }
    else fs.unlinkSync(file);
  }
}
try {
  fs.mkdirSync(installed);
  for (const name of ['bin', 'src', 'engine', 'vendor', 'scripts', 'skills', 'docs', 'plugin.json', 'package.json', 'package-lock.json', 'README.md', 'LICENSE', 'NOTICE', 'node_modules']) {
    fs.cpSync(path.join(packageRoot, name), path.join(installed, name), { recursive: true, errorOnExist: true, force: false, dereference: false });
  }
  const require = createRequire(path.join(installed, 'package.json'));
  for (const dep of ['@xmldom/xmldom', 'jszip', 'playwright-core', 'zod']) assert.ok(require.resolve(dep).startsWith(installed + path.sep), dep);
  function cli(args) {
    const r = spawnSync(process.execPath, [path.join(installed, 'bin', 'autosw.mjs'), ...args], { cwd: installed, encoding: 'utf8', timeout: 90000, env: { ...process.env, PATH: path.dirname(process.execPath) + path.delimiter + process.env.PATH } });
    assert.ifError(r.error); assert.equal(r.status, 0, r.stderr); return JSON.parse(r.stdout);
  }
  cli(['init', '--workspace', work]);
  cli(['write-draft', '--workspace', work, '--output', 'drafts/input.md', '--content', '# 독립 설치 시험\n\n상위 프로젝트 없이 작성합니다.']);
  const p = cli(['preview', '--workspace', work, '--kind', 'create', '--input', 'drafts/input.md', '--output', 'outputs/result.hwpx']);
  cli(['apply', '--workspace', work, '--plan', p.planId, '--confirm', p.confirmHash]);
  assert.equal(cli(['verify', '--workspace', work, '--plan', p.planId]).independentCheck, 'passed');
  cli(['write-draft', '--workspace', work, '--output', 'drafts/ledger.json', '--content', JSON.stringify({ operation: 'add', kind: 'expense', data: { program: '독립 합성', date: '2026-11-09', entryKey: 'receipt-1', item: '재료', amount: 1234 } })]);
  const lp = cli(['ledger-preview', '--workspace', work, '--input', 'drafts/ledger.json']);
  cli(['ledger-apply', '--workspace', work, '--plan', lp.planId, '--confirm', lp.confirmHash, '--approved']);
  assert.equal(cli(['ledger-report', '--workspace', work]).totals.expenseTotal, 1234);
  const moved = path.join(base, '옮긴 개인 작업 #');
  fs.renameSync(work, moved);
  assert.equal(cli(['ledger-report', '--workspace', moved]).totals.expenseTotal, 1234);
  assert.ok(fs.existsSync(path.join(moved, 'outputs/result.hwpx')));
  console.log('독립 설치: 자체 의존성·HWPX 구조 대조·폴더 이동 후 장부 조회 통과');
} finally {
  if (path.dirname(base).toLowerCase() !== fs.realpathSync(os.tmpdir()).toLowerCase()) throw new Error('정리 대상이 임시폴더 밖입니다.');
  clean(base); fs.rmdirSync(base);
}
