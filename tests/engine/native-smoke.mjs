// 선택적 Windows/한글 실검증. 일반 npm test에서는 호출하지 않는다.
// --review-dir 는 새 OS 임시 폴더만 허용하며, 호출자가 전 페이지 검토 후 산출물을 정리한다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { composeHwpx } from '../../engine/hwpx-compose.mjs';
import { loadHwpx, sha256 } from '../../engine/hwpx-document.mjs';
import { validateIndependent } from '../../engine/hwpx-form.mjs';

if (process.platform !== 'win32') throw new Error('이 검증은 Windows와 설치된 한글이 필요합니다.');
const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== '--review-dir') throw new Error('사용법: node tests/engine/native-smoke.mjs --review-dir 새OS임시폴더');
const directory = path.resolve(args[1]);
if (path.dirname(directory).toLowerCase() !== fs.realpathSync(os.tmpdir()).toLowerCase() || fs.existsSync(directory)) throw new Error('새 OS 임시 폴더를 지정하세요.');
fs.mkdirSync(directory);
const source = path.join(directory, 'source.hwpx');
const bytes = composeHwpx({ schemaVersion: 1, title: 'autosw 합성 문서 검증', blocks: [
  { type: 'heading', level: 1, text: 'autosw 한글 출력 검증' },
  { type: 'paragraph', text: '실제 업무 자료가 없는 합성 문서입니다.\n한글, 숫자 12345, 기호 < & >를 확인합니다.' },
  { type: 'table', rows: [['항목', '확인값'], ['합성 자료', '123'], ['상태', '확인 중']] },
  { type: 'pageBreak' },
  { type: 'heading', level: 2, text: '두 번째 페이지' },
  { type: 'paragraph', text: '본문 누락과 표 잘림, 불필요한 빈 쪽이 없는지 확인합니다.' },
] }).bytes;
fs.writeFileSync(source, bytes, { flag: 'wx' });
const original = loadHwpx(bytes).structure;
const texts = structure => structure.sections.flatMap(s => s.paragraphs.map(p => p.text));
const tables = structure => structure.sections.flatMap(s => s.tables.map(t => ({ rows: t.rows, cols: t.cols, cells: t.cells.map(c => ({ row: c.row, col: c.col, rowSpan: c.rowSpan, colSpan: c.colSpan })) })));
const expected = texts(original);
const helper = fileURLToPath(new URL('../../engine/hangul-verify.ps1', import.meta.url));
const reports = [];
for (const format of ['HWPX', 'HWP']) {
  const output = path.join(directory, `checked.${format.toLowerCase()}`);
  const pdf = path.join(directory, `${format.toLowerCase()}.pdf`);
  const verification = format === 'HWP' ? path.join(directory, 'hwp-roundtrip.hwpx') : output;
  const scriptArgs = ['-NoProfile', '-File', helper, '-InputPath', source, '-OutputPath', output, '-PdfPath', pdf, '-OutputFormat', format];
  if (format === 'HWP') scriptArgs.push('-VerificationHwpxPath', verification);
  const run = spawnSync('powershell.exe', scriptArgs, { encoding: 'utf8', windowsHide: true, timeout: 60000 });
  assert.ifError(run.error);
  assert.equal(run.status, 0, `${run.stdout}\n${run.stderr}`);
  const report = JSON.parse(run.stdout.replace(/^\uFEFF/, ''));
  assert.equal(report.status, 'awaiting-visual-review');
  assert.equal(report.savedPages, 2);
  assert.equal(sha256(fs.readFileSync(source)), sha256(bytes));
  const checked = fs.readFileSync(verification);
  validateIndependent(checked);
  const checkedStructure = loadHwpx(checked).structure;
  const actual = texts(checkedStructure);
  assert.deepEqual(actual, expected, `${format} 재저장 후 본문이 달라졌습니다.`);
  assert.deepEqual(tables(checkedStructure), tables(original), `${format} 재저장 후 표 구조가 달라졌습니다.`);
  assert.equal(fs.readFileSync(pdf).subarray(0, 5).toString(), '%PDF-');
  reports.push({ ...report, contentCheck: 'passed', independentCheck: 'passed', paragraphs: actual.length, emptyParagraphs: actual.filter(text => !text).length, tables: tables(checkedStructure).length });
}
console.log(JSON.stringify({ status: 'native-content-passed-awaiting-visual-review', directory, reports }, null, 2));
