import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { initialize, safePath, workspaceRoot, packageRoot } from '../src/workspace.mjs';
import { doctor } from '../src/doctor.mjs';
import * as service from '../src/service.mjs';
import { composeHwpx } from '../engine/hwpx-compose.mjs';
import { sha256 } from '../engine/hwpx-document.mjs';

const base = fs.mkdtempSync(path.join(os.tmpdir(), 'autosw-service-'));
const root = path.join(base, '한글 # 작업'), outside = path.join(base, 'outside');
function cleanup(dir) {
  // Only the freshly created, recorded test directory is traversed; links are unlinked.
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const item = path.join(dir, entry.name);
    if (entry.isDirectory() && !entry.isSymbolicLink()) { cleanup(item); fs.rmdirSync(item); }
    else fs.unlinkSync(item);
  }
}
try {
  initialize(root); fs.mkdirSync(outside); fs.writeFileSync(path.join(outside, 'secret.md'), 'not-readable');
  assert.throws(() => workspaceRoot(packageRoot));
  assert.throws(() => safePath(root, '../outside/secret.md'));
  fs.writeFileSync(path.join(root, '.env'), 'synthetic');
  assert.throws(() => service.readDraft(root, '.env'));
  fs.symlinkSync(outside, path.join(root, 'drafts', 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => service.readDraft(root, 'drafts/linked/secret.md'));
  assert.throws(() => service.writeDraft(root, 'drafts/linked/new.md', 'escaped'));
  fs.mkdirSync(path.join(root, '.local'));
  fs.writeFileSync(path.join(root, '.local', 'private.json'), '{}');
  fs.symlinkSync(path.join(root, '.local'), path.join(root, 'drafts', 'hidden-alias'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => service.readDraft(root, 'drafts/hidden-alias/private.json'));
  assert.throws(() => service.writeDraft(root, 'drafts/hidden-alias/new.md', 'hidden write'));
  const draft = service.writeDraft(root, 'drafts/원고.md', '# 시험 보고\n\n합성 본문입니다.\n');
  assert.equal(service.readDraft(root, 'drafts/원고.md').content.includes('합성'), true);
  assert.throws(() => service.writeDraft(root, 'drafts/원고.md', 'overwrite'));
  const p = await service.preview(root, { kind: 'create', input: 'drafts/원고.md', output: 'outputs/생성.hwpx' });
  assert.equal(p.status, 'awaiting-content-confirmation');
  assert.throws(() => service.apply(root, p.planId, '0'.repeat(64)));
  const applied = service.apply(root, p.planId, p.confirmHash);
  assert.match(applied.status, /awaiting/);
  assert.equal(service.verify(root, p.planId).independentCheck, 'passed');
  assert.throws(() => service.apply(root, p.planId, p.confirmHash));
  fs.appendFileSync(draft.path, '원고 변경');
  assert.throws(() => service.verify(root, p.planId));
  const original = composeHwpx({ schemaVersion: 1, blocks: [{ type: 'heading', level: 1, text: '합성 양식' }, { type: 'paragraph', text: '기존 문구' }] }).bytes;
  fs.writeFileSync(path.join(root, 'templates', '양식.hwpx'), original);
  const structure = service.inspect(root, 'templates/양식.hwpx');
  const target = structure.targets.find(t => t.writable && t.before.includes('기존 문구'));
  assert.ok(target, JSON.stringify(structure.targets));
  const request = { schemaVersion: 1, sourceSha256: structure.sourceSha256, changes: [{ target: target.key, label: '본문', source: '합성 원고', action: 'set', expected: target.before, values: ['변경 문구'] }], requiredTargets: [target.key], unmapped: [] };
  service.writeDraft(root, 'drafts/입력.json', JSON.stringify(request));
  const fill = await service.preview(root, { kind: 'fill', input: 'templates/양식.hwpx', request: 'drafts/입력.json', output: 'outputs/채움.hwpx' });
  service.apply(root, fill.planId, fill.confirmHash);
  assert.equal(sha256(fs.readFileSync(path.join(root, 'templates', '양식.hwpx'))), sha256(original));
  assert.equal(service.verify(root, fill.planId).independentCheck, 'passed');
  // Tampering with stored plan paths must never redirect reads to an external file.
  const planFile = path.join(root, '.autosw', 'plans', fill.planId + '.json');
  const stored = JSON.parse(fs.readFileSync(planFile, 'utf8')); stored.plan.sourcePath = path.join(outside, 'secret.md');
  fs.writeFileSync(planFile, JSON.stringify(stored));
  assert.throws(() => service.apply(root, fill.planId, fill.confirmHash));
  assert.throws(() => service.publish(root, { visualReviewed: false }));
  const configPath = path.join(root, 'autosw.config.json');
  const configBefore = fs.readFileSync(configPath);
  initialize(root);
  assert.deepEqual(fs.readFileSync(configPath), configBefore);
  assert.equal(doctor(root).workspaceInitialized, true);
  const cli = spawnSync(process.execPath, [path.join(packageRoot, 'bin', 'autosw.mjs'), 'list', '--workspace', root], { encoding: 'utf8' });
  assert.equal(cli.status, 0, cli.stderr); assert.ok(JSON.parse(cli.stdout).files.length >= 4);
  console.log('service: 폴더 경계·원고·생성·양식 채움·변경 차단·설정 보존 통과');
} finally { cleanup(base); fs.rmdirSync(base); }
