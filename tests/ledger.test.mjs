import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync, spawn } from 'node:child_process';
import { initialize, packageRoot } from '../src/workspace.mjs';
import { ledgerPreview, ledgerApply, ledgerQuery, ledgerReport } from '../src/ledger.mjs';
import * as service from '../src/service.mjs';
import { inspectHwpx } from '../engine/hwpx-document.mjs';

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'autosw-ledger-'));
initialize(root);
const session = (session, participantIds, program = '합성 프로그램') => ({ operation: 'add', kind: 'attendance', data: { program, date: '2026-11-09', session, count: participantIds.length, participantIds } });
const spend = (entryKey, amount = 1500) => ({ operation: 'add', kind: 'expense', data: { program: '합성 프로그램', date: '2026-11-09', entryKey, item: '합성 재료', amount } });
const apply = p => ledgerApply(root, { planId: p.planId, confirmHash: p.confirmHash, approved: true });
const save = r => { const p = ledgerPreview(root, r); apply(p); return p; };
const ledgerFile = path.join(root, '.autosw', 'ledger.json');
function clean(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, e.name);
    if (e.isDirectory() && !e.isSymbolicLink()) { clean(file); fs.rmdirSync(file); } else fs.unlinkSync(file);
  }
}
try {
  assert.equal(ledgerQuery(root).totals.uniqueParticipants, 0);
  assert.equal(fs.existsSync(ledgerFile), false, '조회는 장부를 생성하지 않음');
  const first = ledgerPreview(root, session('1', ['p1', 'p2']));
  assert.equal(fs.existsSync(ledgerFile), false, '미리보기는 실적 미반영');
  assert.throws(() => ledgerApply(root, { planId: first.planId, confirmHash: first.confirmHash }), /승인/);
  assert.throws(() => apply({ ...first, confirmHash: '0'.repeat(64) }), /변경/);
  apply(first);
  assert.equal(apply(first).status, 'already-applied');
  assert.equal(ledgerQuery(root).records.attendance.length, 1);
  assert.throws(() => ledgerPreview(root, session('1', ['p3'])), /이미/);
  save(session('2', ['p2', 'p3'])); save(session('1', ['p1'], '다른 합성 프로그램'));
  let q = ledgerQuery(root);
  assert.equal(q.totals.totalAttendance, 5); assert.equal(q.totals.uniqueParticipants, 3);
  assert.deepEqual(q.byProgram.map(r => r.uniqueParticipants).sort(), [1, 3]);
  const counted = save({ operation: 'add', kind: 'attendance', data: { program: '합성 프로그램', date: '2026-11-10', session: '3', count: 4 } });
  assert.equal(ledgerQuery(root).totals.uniqueParticipants, null);
  assert.equal(ledgerQuery(root, { from: '2026-11-09', to: '2026-11-09', program: '합성 프로그램' }).totals.uniqueParticipants, 3);
  const zero = { operation: 'add', kind: 'attendance', data: { program: '합성 프로그램', date: '2026-11-10', session: 'zero', count: 0 } };
  save(zero);
  const exp = save(spend('영수증-1/행-1'));
  save(spend('영수증-1/행-2', 2000));
  assert.equal(ledgerQuery(root).totals.expenseTotal, 3500);
  assert.throws(() => ledgerPreview(root, spend('영수증-1/행-1')), /이미/);
  const stale = ledgerPreview(root, spend('오래된 미리보기'));
  save(spend('새 변경', 500));
  assert.throws(() => apply(stale), /장부가 변경/);
  const update = save({ operation: 'update', kind: 'expense', id: exp.after.id, data: { ...spend('영수증-1/행-1', 1800).data }, reason: '합성 금액 정정' });
  assert.equal(update.before.amount, 1500); assert.equal(update.after.amount, 1800);
  assert.equal(ledgerQuery(root).totals.expenseTotal, 4300);
  const deleted = save({ operation: 'delete', kind: 'attendance', id: counted.after.id, reason: '합성 취소' });
  assert.equal(deleted.after, null);
  assert.equal(ledgerQuery(root).totals.uniqueParticipants, 3);
  assert.equal(apply(deleted).status, 'already-applied');
  assert.equal(apply(counted).status, 'already-applied', '삭제 후 과거 등록 요청은 부활하지 않음');
  const json = JSON.parse(fs.readFileSync(ledgerFile));
  assert.ok(json.history.some(h => h.operation === 'delete' && h.before.id === counted.after.id));
  const beforeInvalid = fs.readFileSync(ledgerFile, 'utf8');
  for (const bad of [
    { ...spend('bad'), data: { ...spend('bad').data, amount: -1 } },
    { ...spend('bad'), data: { ...spend('bad').data, amount: 1.2 } },
    { ...spend('bad'), data: { ...spend('bad').data, amount: Number.MAX_SAFE_INTEGER } },
    { ...spend('bad'), data: { ...spend('bad').data, date: '2026-02-29' } },
    { ...session('bad', ['p1']), data: { ...session('bad', ['p1']).data, count: 3 } },
    session('bad', ['p1', 'p1']),
    { ...spend('bad'), unknown: true },
    { ...spend('bad'), kind: 'attendance' }
  ]) assert.throws(() => ledgerPreview(root, bad));
  assert.throws(() => ledgerQuery(root, { from: '2026-11-10', to: '2026-11-09' }));
  assert.equal(fs.readFileSync(ledgerFile, 'utf8'), beforeInvalid);
  const tampered = ledgerPreview(root, spend('tamper'));
  const planFile = path.join(root, '.autosw', `ledger-plan-${tampered.planId}.json`);
  const plan = JSON.parse(fs.readFileSync(planFile)); plan.after.amount = 999;
  fs.writeFileSync(planFile, JSON.stringify(plan));
  assert.throws(() => apply(tampered), /変更|변경/);
  const lockPlan = ledgerPreview(root, spend('locked'));
  const lockFile = path.join(root, '.autosw', 'ledger.lock'); fs.writeFileSync(lockFile, '');
  assert.throws(() => apply(lockPlan), /다른 장부 쓰기/); fs.unlinkSync(lockFile);
  // Two independent processes race for the same approved plan: at most one write.
  function childApply() {
    return new Promise(resolve => {
      const child = spawn(process.execPath, [path.join(packageRoot, 'bin/autosw.mjs'), 'ledger-apply', '--workspace', root, '--plan', lockPlan.planId, '--confirm', lockPlan.confirmHash, '--approved'], { windowsHide: true });
      let out = '', err = ''; child.stdout.on('data', b => out += b); child.stderr.on('data', b => err += b);
      child.on('error', e => resolve({ code: -1, err: e.message })); child.on('close', code => resolve({ code, out, err }));
    });
  }
  const race = await Promise.all([childApply(), childApply()]);
  assert.ok(race.some(r => r.code === 0));
  assert.ok(race.every(r => r.code === 0 || /다른 장부 쓰기/.test(r.err)));
  assert.equal(ledgerQuery(root).records.expense.filter(r => r.entryKey === 'locked').length, 1);
  assert.equal(apply(lockPlan).status, 'already-applied');
  const report = ledgerReport(root, { program: '합성 프로그램', from: '2026-11-01', to: '2026-11-30' });
  const expenseOnly = ledgerReport(root, { kind: 'expense' });
  assert.equal(expenseOnly.totals.totalAttendance, null);
  assert.match(expenseOnly.markdown, /인원 실적 미조회/);
  assert.doesNotMatch(expenseOnly.markdown, /연인원:|실인원:|회기 수:/);
  assert.doesNotMatch(ledgerReport(root, { kind: 'attendance' }).markdown, /지출: 0/);
  assert.match(report.markdown, /연인원: 4명/); assert.match(report.markdown, /지출: 5800원/);
  const special = 'A-B *교육* | <합성> [이름]';
  save(session('1', ['p4'], special));
  const exact = ledgerReport(root, { program: special });
  service.writeDraft(root, 'drafts/집계.json', JSON.stringify(exact.document));
  const document = await service.preview(root, { kind: 'create', input: 'drafts/집계.json', output: 'outputs/집계.hwpx' });
  service.apply(root, document.planId, document.confirmHash);
  assert.equal(service.verify(root, document.planId).independentCheck, 'passed');
  assert.ok(JSON.stringify(inspectHwpx(fs.readFileSync(path.join(root, 'outputs/집계.hwpx')))).includes(special));
  const cli = spawnSync(process.execPath, [path.join(packageRoot, 'bin/autosw.mjs'), 'ledger-query', '--workspace', root, '--kind', 'expense'], { encoding: 'utf8', windowsHide: true });
  assert.equal(cli.status, 0, cli.stderr); assert.equal(JSON.parse(cli.stdout).totals.expenseTotal, 5800);
  const backup = fs.readFileSync(ledgerFile);
  fs.unlinkSync(ledgerFile);
  const outside = path.join(root, 'drafts', 'linked.json'); fs.writeFileSync(outside, backup);
  fs.linkSync(outside, ledgerFile);
  assert.throws(() => ledgerQuery(root), /연결된 장부/);
  fs.unlinkSync(ledgerFile); fs.writeFileSync(ledgerFile, '{broken');
  assert.throws(() => ledgerQuery(root));
  console.log('장부: 승인·중복·정정·삭제·재시도·기간·실/연인원·금액·변조·잠금·동시 프로세스·CLI·문서 연결 통과');
} finally {
  if (path.dirname(root).toLowerCase() !== fs.realpathSync(os.tmpdir()).toLowerCase()) throw new Error('정리 대상 확인 실패');
  clean(root); fs.rmdirSync(root);
}
