// Optional actual Hancom integration. Create a review directory; after visually checking
// every PDF page, run --finish with that same directory to test publication and clean up.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { initialize } from '../src/workspace.mjs';
import * as service from '../src/service.mjs';
import { sha256 } from '../engine/hwpx-document.mjs';

const [action, folder] = process.argv.slice(2);
const root = path.resolve(folder || '');
if (!['--prepare', '--finish'].includes(action) || path.dirname(root).toLowerCase() !== fs.realpathSync(os.tmpdir()).toLowerCase() || !path.basename(root).startsWith('autosw-native-service-')) throw new Error('새 OS 임시 폴더 autosw-native-service-<고유값>을 지정하세요.');
if (action === '--prepare') {
  if (fs.existsSync(root)) throw new Error('새 임시 폴더를 사용하세요.');
  initialize(root);
  service.writeDraft(root, 'drafts/input.json', JSON.stringify({ schemaVersion: 1, title: 'autosw 통합 시험', blocks: [
    { type: 'heading', level: 1, text: 'autosw 문서 작성·출력 시험' },
    { type: 'paragraph', text: '실제 업무 자료가 없는 합성 문서입니다. 한글과 숫자 12345를 확인합니다.' },
    { type: 'table', rows: [['항목', '확인값'], ['합성 자료', '123'], ['상태', '검토 중']] },
    { type: 'pageBreak' },
    { type: 'heading', level: 2, text: '두 번째 페이지' },
    { type: 'paragraph', text: '표와 본문 누락, 잘림, 빈 페이지가 없는지 확인합니다.' }
  ] }));
  const plan = await service.preview(root, { kind: 'create', input: 'drafts/input.json', output: 'outputs/generated.hwpx' });
  service.apply(root, plan.planId, plan.confirmHash);
  const reports = [];
  for (const format of ['HWPX', 'HWP']) {
    const report = service.checkHangul(root, { input: 'outputs/generated.hwpx', output: `outputs/checked.${format.toLowerCase()}`, pdf: `previews/${format}.pdf`, planId: plan.planId, format, verification: 'previews/hwp-roundtrip.hwpx' });
    assert.equal(report.contentCheck, 'passed'); assert.equal(report.savedPages, 2);
    reports.push(report);
  }
  fs.writeFileSync(path.join(root, 'review.json'), JSON.stringify({ marker: 'autosw-native-service-v1', reports }, null, 2));
  console.log(JSON.stringify({ status: 'awaiting-real-visual-review', root, reports }, null, 2));
} else {
  const record = JSON.parse(fs.readFileSync(path.join(root, 'review.json'), 'utf8'));
  assert.equal(record.marker, 'autosw-native-service-v1');
  for (const report of record.reports) {
    const extension = report.format.toLowerCase();
    assert.throws(() => service.publish(root, { reportId: report.reportId, reportHash: report.reportHash, output: `outputs/final.${extension}`, visualReviewed: false }));
    const published = service.publish(root, { reportId: report.reportId, reportHash: report.reportHash, output: `outputs/final.${extension}`, visualReviewed: true });
    assert.equal(sha256(fs.readFileSync(published.path)), report.outputSha256);
  }
  // A changed PDF must invalidate a previously reviewed report.
  const report = record.reports[0];
  const pdfBytes = fs.readFileSync(report.pdf);
  fs.appendFileSync(report.pdf, '\nchanged');
  assert.throws(() => service.publish(root, { reportId: report.reportId, reportHash: report.reportHash, output: 'outputs/not-published.hwpx', visualReviewed: true }));
  fs.writeFileSync(report.pdf, pdfBytes);
  const unrelated = path.join(root, 'outputs', 'unrelated.hwpx');
  fs.copyFileSync(report.source, unrelated);
  assert.throws(() => service.publish(root, { reportId: report.reportId, reportHash: report.reportHash, output: 'outputs/unrelated.hwpx', expectedHash: report.sourceSha256, visualReviewed: true }));
  assert.throws(() => service.publish(root, { reportId: report.reportId, reportHash: report.reportHash, output: 'outputs/generated.hwpx', expectedHash: '0'.repeat(64), visualReviewed: true }));
  service.publish(root, { reportId: report.reportId, reportHash: report.reportHash, output: 'outputs/generated.hwpx', expectedHash: report.sourceSha256, visualReviewed: true });
  assert.equal(sha256(fs.readFileSync(report.source)), report.outputSha256);
  function clean(dir) {
    for (const item of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, item.name);
      if (item.isDirectory() && !item.isSymbolicLink()) { clean(full); fs.rmdirSync(full); }
      else fs.unlinkSync(full);
    }
  }
  clean(root); fs.rmdirSync(root);
  console.log('실제 한글 HWPX/HWP 내용대조·검토후 저장·검증변경 차단 통과. 검사용 파일 정리 완료.');
}
