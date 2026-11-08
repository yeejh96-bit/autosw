// 운영 문서에 접근하지 않고 합성 HWPX로 공통 작성 경로를 검증한다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateRawSync } from 'node:zlib';
import { spawnSync } from 'node:child_process';
import { loadHwpx, prepareHwpx, verifyHwpx, sha256 } from '../../engine/hwpx-document.mjs';
import { readZipStrict, patchZip } from '../../engine/office-zip.mjs';
import { createPlan, applyPlan, validatePlan } from '../../engine/hwpx-form.mjs';

const scripts = fileURLToPath(new URL('../../engine/', import.meta.url));
const formScript = path.join(scripts, 'hwpx-form.mjs');
const formPowerShell = path.join(scripts, 'hwpx-form.ps1');
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'hwpx-form-test-'));
const hp = 'http://www.hancom.co.kr/hwpml/2011/paragraph';
const section = body => `<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="${hp}">${body}</hs:sec>`;
const p = (text, style = '0') => `<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run charPrIDRef="${style}"><hp:t>${text}</hp:t></hp:run><hp:linesegarray><hp:lineseg textpos="0"/></hp:linesegarray></hp:p>`;
const cell = (row, col, body, rowSpan = 1, colSpan = 1) => `<hp:tc><hp:subList>${body}</hp:subList><hp:cellAddr rowAddr="${row}" colAddr="${col}"/><hp:cellSpan rowSpan="${rowSpan}" colSpan="${colSpan}"/></hp:tc>`;
const table = (rows, cols, body, id = '101') => `<hp:p><hp:run><hp:tbl id="${id}" rowCnt="${rows}" colCnt="${cols}">${body}</hp:tbl></hp:run></hp:p>`;

// 시험 입력을 독립 생성한다. 운영 ZIP writer를 호출해 검증 입력을 만들지 않는다.
function zip(entries, { method = 8, descriptor = false } = {}) {
  const chunks = [], records = []; let offset = 0;
  for (const [name, value] of entries) {
    const filename = Buffer.from(name), data = Buffer.isBuffer(value) ? value : Buffer.from(value);
    let crc = 0xffffffff;
    for (const byte of data) {
      crc ^= byte;
      for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
    crc = (crc ^ 0xffffffff) >>> 0;
    const mode = name === 'mimetype' ? 0 : method;
    const packed = mode === 0 ? data : deflateRawSync(data);
    const flags = descriptor ? 8 : 0;
    const local = Buffer.alloc(30), central = Buffer.alloc(46);
    local.writeUInt32LE(0x04034b50); local.writeUInt16LE(20, 4); local.writeUInt16LE(flags, 6); local.writeUInt16LE(mode, 8);
    if (!descriptor) { local.writeUInt32LE(crc, 14); local.writeUInt32LE(packed.length, 18); local.writeUInt32LE(data.length, 22); }
    local.writeUInt16LE(filename.length, 26);
    central.writeUInt32LE(0x02014b50); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6);
    central.writeUInt16LE(flags, 8); central.writeUInt16LE(mode, 10);
    central.writeUInt32LE(crc, 16); central.writeUInt32LE(packed.length, 20); central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(filename.length, 28); central.writeUInt32LE(offset, 42);
    const trailer = descriptor ? Buffer.alloc(16) : Buffer.alloc(0);
    if (descriptor) { trailer.writeUInt32LE(0x08074b50); trailer.writeUInt32LE(crc, 4); trailer.writeUInt32LE(packed.length, 8); trailer.writeUInt32LE(data.length, 12); }
    chunks.push(local, filename, packed, trailer); records.push(central, filename);
    offset += local.length + filename.length + packed.length + trailer.length;
  }
  const central = Buffer.concat(records), end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50); end.writeUInt16LE(entries.length, 8); end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(central.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...chunks, central, end]);
}
const entries = xml => [
  ['mimetype', 'application/hwp+zip'], ['Contents/header.xml', '<header><style id="0"/></header>'],
  ['Contents/section0.xml', xml], ['BinData/untouched.bin', Buffer.from([0, 255, 17, 32])],
  ['Preview/PrvText.txt', '이전 미리보기'], ['Preview/PrvImage.png', Buffer.from([137, 80, 78, 71])],
];
const fixture = (xml, options) => zip(entries(xml), options);
const paragraphDoc = fixture(section(p('처음 본 문단 양식') + p('바꾸지 않을 본문')));
const mergedXml = section(table(2, 2, `<hp:tr>${cell(0, 0, p('항목'), 2)}${cell(0, 1, p(''))}</hp:tr><hp:tr>${cell(1, 1, p('두 번째 빈칸'))}</hp:tr>`));
const mergedDoc = fixture(mergedXml);
const nestedXml = section(p('혼합 문서 제목') + table(1, 2, `<hp:tr>${cell(0, 0, p('총액'))}${cell(0, 1, table(1, 1, `<hp:tr>${cell(0, 0, p('중첩 입력'))}</hp:tr>`, '202'))}</hp:tr>`));
const nestedDoc = fixture(nestedXml);
function change(bytes, target, values, extras = {}) {
  const before = loadHwpx(bytes).targets.get(target)?.before;
  assert.ok(before, `test target missing: ${target}`);
  return { target, label: '시험 항목', source: '비식별 합성 시험값', action: 'set', expected: before, values, ...extras };
}
const request = (bytes, changes) => ({ schemaVersion: 1, sourceSha256: sha256(bytes), changes });
const clone = value => structuredClone(value);
let cases = 0;
function test(name, fn) { try { fn(); cases++; } catch (error) { error.message = `${name}: ${error.message}`; throw error; } }
function runNode(file, args) { return spawnSync(process.execPath, [file, ...args], { cwd: temp, encoding: 'utf8', windowsHide: true, timeout: 30000 }); }
function runPs(file, args) { return spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', file, ...args], { cwd: temp, encoding: 'utf8', windowsHide: true, timeout: 30000 }); }
function success(result) { assert.ifError(result.error); assert.equal(result.status, 0, result.stdout + result.stderr); return result.stdout; }
function failure(result) { assert.ifError(result.error); assert.notEqual(result.status, 0, result.stdout + result.stderr); }

try {
  test('문단 중심 미등록 양식과 긴 본문·줄바꿈·특수문자', () => {
    const text = '긴 본문 '.repeat(1500) + '\n둘째 줄 < & > " 한글 😀';
    const req = request(paragraphDoc, [change(paragraphDoc, 'Contents/section0.xml/p0', [text])]);
    const result = prepareHwpx(paragraphDoc, req), after = loadHwpx(result.output);
    assert.equal(after.structure.sections[0].paragraphs[0].text, text);
    assert.equal(after.structure.sections[0].paragraphs[1].text, '바꾸지 않을 본문');
    assert.equal(result.report.visualStatus, 'not-checked');
    assert.equal(result.report.previewStatus, 'requires-hangul-save');
    assert.equal(verifyHwpx(paragraphDoc, req, result.output).valueCheck, 'passed');
  });
  test('병합 표·빈칸 작성과 부모 중복 없음', () => {
    const loaded = loadHwpx(mergedDoc), structure = loaded.structure.sections[0];
    assert.equal(structure.tables[0].cells[0].rowSpan, 2);
    assert.equal(structure.paragraphs[0].text, '');
    const target = 'Contents/section0.xml/tbl0/r0c1';
    assert.deepEqual(loaded.targets.get(target).before, ['']);
    const result = prepareHwpx(mergedDoc, request(mergedDoc, [change(mergedDoc, target, ['빈칸 입력'])]));
    assert.deepEqual(loadHwpx(result.output).targets.get(target).before, ['빈칸 입력']);
    assert.equal(loadHwpx(result.output).structure.sections[0].tables[0].cells.length, 3);
  });
  test('빈 run과 self-closing 텍스트·여러 문단 셀 작성', () => {
    const xml = section(table(1, 1, `<hp:tr>${cell(0, 0,
      '<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"/></hp:p>' +
      '<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"><hp:t/></hp:run></hp:p>' +
      '<hp:p paraPrIDRef="0"><hp:run charPrIDRef="0"></hp:run></hp:p>')}</hp:tr>`));
    const bytes = fixture(xml), target = 'Contents/section0.xml/tbl0/r0c0';
    const values = ['빈 run <&>\n줄바꿈', '빈 텍스트', '닫는 run'];
    const result = prepareHwpx(bytes, request(bytes, [change(bytes, target, values)]));
    assert.deepEqual(loadHwpx(result.output).targets.get(target).before, values);
    assert.equal(loadHwpx(result.output).structure.sections[0].paragraphs.length, 4);
  });
  test('혼합·중첩표 말단 작성과 부모 셀 거부', () => {
    const inner = 'Contents/section0.xml/tbl1/r0c0';
    const outer = 'Contents/section0.xml/tbl0/r0c1';
    const result = prepareHwpx(nestedDoc, request(nestedDoc, [change(nestedDoc, inner, ['중첩표 확인'])]));
    assert.deepEqual(loadHwpx(result.output).targets.get(inner).before, ['중첩표 확인']);
    assert.equal(loadHwpx(result.output).structure.sections[0].tables[1].parentCell, outer);
    assert.throws(() => prepareHwpx(nestedDoc, request(nestedDoc, [change(nestedDoc, outer, ['부모 지우기'])])), /지원하지/);
  });
  test('수정 대상 밖 ZIP 바이트와 문단 서식 보존', () => {
    const target = 'Contents/section0.xml/p0';
    const result = prepareHwpx(paragraphDoc, request(paragraphDoc, [change(paragraphDoc, target, ['변경 내용'])]));
    const original = readZipStrict(paragraphDoc), after = readZipStrict(result.output);
    for (const entry of original.entries.filter(e => e.name !== 'Contents/section0.xml')) {
      assert.deepEqual(after.entries.find(e => e.name === entry.name).record, entry.record, entry.name);
    }
    const xml = after.read('Contents/section0.xml').toString();
    assert.ok(xml.includes('paraPrIDRef="0" styleIDRef="0"'));
    assert.equal((xml.match(/<hp:linesegarray>/g) || []).length, 1, '수정하지 않은 문단의 배치 정보 보존');
    assert.deepEqual(original.read('Preview/PrvText.txt'), after.read('Preview/PrvText.txt'));
  });
  test('수정한 본문 XML의 기존 BOM 보존', () => {
    const bytes = fixture('\uFEFF' + section(p('BOM 원문')));
    const result = prepareHwpx(bytes, request(bytes, [change(bytes, 'Contents/section0.xml/p0', ['BOM 변경'])]));
    assert.equal(readZipStrict(result.output).read('Contents/section0.xml').subarray(0, 3).toString('hex'), 'efbbbf');
    assert.equal(loadHwpx(result.output).structure.sections[0].paragraphs[0].text, 'BOM 변경');
  });
  test('보호된 부모 셀의 중첩표 수정 차단', () => {
    const protectedDoc = fixture(nestedXml.replace('<hp:tc><hp:subList><hp:p><hp:run><hp:tbl', '<hp:tc protect="1"><hp:subList><hp:p><hp:run><hp:tbl'));
    const target = 'Contents/section0.xml/tbl1/r0c0';
    assert.equal(loadHwpx(protectedDoc).targets.get(target).writable, false);
    assert.throws(() => prepareHwpx(protectedDoc, request(protectedDoc, [change(protectedDoc, target, ['보호 위반'])])), /보호된/);
    assert.deepEqual(prepareHwpx(protectedDoc, request(protectedDoc, [change(protectedDoc, target, undefined, { action: 'preserve' })])).output, protectedDoc);
  });
  test('무압축·데이터 설명자 ZIP의 수정 후 다시 읽기', () => {
    for (const options of [{ method: 0 }, { method: 8, descriptor: true }]) {
      const bytes = fixture(section(p('압축 시험')), options);
      const result = prepareHwpx(bytes, request(bytes, [change(bytes, 'Contents/section0.xml/p0', ['압축 변경'])]));
      assert.equal(loadHwpx(result.output).structure.sections[0].paragraphs[0].text, '압축 변경');
      assert.deepEqual(readZipStrict(result.output).entries[0].record, readZipStrict(bytes).entries[0].record);
    }
  });
  test('중복 대상 및 셀·문단 중첩 변경 거부', () => {
    const target = 'Contents/section0.xml/tbl0/r0c1', update = change(mergedDoc, target, ['변경']);
    assert.throws(() => prepareHwpx(mergedDoc, request(mergedDoc, [update, update])), /중복/);
    const child = loadHwpx(mergedDoc).structure.sections[0].tables[0].cells[1].paragraphs[0];
    assert.throws(() => prepareHwpx(mergedDoc, request(mergedDoc, [update, change(mergedDoc, child, ['다른 변경'])])), /중복/);
  });
  test('대상 없음·현재값 다름·원본 해시 다름 거부', () => {
    const good = request(paragraphDoc, [change(paragraphDoc, 'Contents/section0.xml/p0', ['새 값'])]);
    const badTarget = clone(good); badTarget.changes[0].target = 'Contents/section0.xml/p999';
    assert.throws(() => prepareHwpx(paragraphDoc, badTarget), /위치/);
    const badExpected = clone(good); badExpected.changes[0].expected = ['틀린 값'];
    assert.throws(() => prepareHwpx(paragraphDoc, badExpected), /현재 값/);
    const badHash = clone(good); badHash.sourceSha256 = '0'.repeat(64);
    assert.throws(() => prepareHwpx(paragraphDoc, badHash), /원본 버전/);
  });
  test('필수 위치 누락·미대응 항목 차단', () => {
    const good = request(paragraphDoc, [change(paragraphDoc, 'Contents/section0.xml/p0', ['확인된 값'])]);
    assert.throws(() => prepareHwpx(paragraphDoc, { ...good, requiredTargets: ['Contents/section0.xml/p1'] }), /필수/);
    assert.throws(() => prepareHwpx(paragraphDoc, { ...good, unmapped: ['위치를 정하지 못한 항목'] }), /정하지/);
    assert.throws(() => prepareHwpx(paragraphDoc, { ...good, unmapped: '잘못된 형식' }), /정하지/);
    assert.equal(prepareHwpx(paragraphDoc, { ...good, requiredTargets: ['Contents/section0.xml/p0'], unmapped: [] }).report.valueCheck, 'passed');
  });
  test('명시한 비우기·보존과 문단 수 검증', () => {
    const target = 'Contents/section0.xml/p0';
    const clear = prepareHwpx(paragraphDoc, request(paragraphDoc, [change(paragraphDoc, target, [''], { action: 'clear' })]));
    assert.equal(loadHwpx(clear.output).structure.sections[0].paragraphs[0].text, '');
    const keep = prepareHwpx(paragraphDoc, request(paragraphDoc, [change(paragraphDoc, target, undefined, { action: 'preserve' })]));
    assert.deepEqual(keep.output, paragraphDoc);
    assert.throws(() => prepareHwpx(paragraphDoc, request(paragraphDoc, [change(paragraphDoc, target, [''])])), /clear/);
    assert.throws(() => prepareHwpx(paragraphDoc, request(paragraphDoc, [change(paragraphDoc, target, ['새 문단', '추가 문단'])])), /문단 수/);
  });
  test('혼합 글자 서식·탭·불법 제어문자·잘못된 요청 거부', () => {
    const mixed = fixture(section('<hp:p><hp:run charPrIDRef="0"><hp:t>보통</hp:t></hp:run><hp:run charPrIDRef="1"><hp:t>강조</hp:t></hp:run></hp:p>'));
    assert.throws(() => prepareHwpx(mixed, request(mixed, [change(mixed, 'Contents/section0.xml/p0', ['교체'])])), /혼합/);
    const preserved = prepareHwpx(mixed, request(mixed, [change(mixed, 'Contents/section0.xml/p0', undefined, { action: 'preserve' })]));
    assert.deepEqual(preserved.output, mixed, '미지원 서식 영역도 명시적 보존 가능');
    for (const text of ['탭\t값', '줄\r\n값', '잘못\u0001된 값']) {
      assert.throws(() => prepareHwpx(paragraphDoc, request(paragraphDoc, [change(paragraphDoc, 'Contents/section0.xml/p0', [text])])));
    }
    const req = request(paragraphDoc, [change(paragraphDoc, 'Contents/section0.xml/p0', ['변경'])]);
    assert.throws(() => prepareHwpx(paragraphDoc, { ...req, unexpected: true }), /잘못된/);
  });
  test('손상·DTD·중복 ZIP·HWP 서명·범위 오류 거부', () => {
    assert.throws(() => loadHwpx(Buffer.from([0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])), /HWP/);
    assert.throws(() => loadHwpx(fixture('<!DOCTYPE sec [<!ENTITY x "bad">]>' + section(p('&x;')))));
    assert.throws(() => loadHwpx(fixture(section(p('본문')).replace('</hp:t>', '</wrong>'))));
    assert.throws(() => loadHwpx(zip([...entries(section(p('본문'))), ['Contents/section0.xml', section(p('중복'))]])), /중복/);
    assert.throws(() => loadHwpx(fixture(mergedXml.replace('colSpan="1"', 'colSpan="3"'))), /표 밖|겹치는/);
    const corrupted = Buffer.from(paragraphDoc); corrupted[50] ^= 1;
    assert.throws(() => loadHwpx(corrupted));
    const declaration = '<?xml version="1.0" encoding="UTF-8"?>';
    for (const badXml of [declaration + declaration + section(p('본문')), '<?xml garbage encoding="UTF-8"?>' + section(p('본문')), section(p('본문')).replace('</hp:p>', '</ hp:p>')]) {
      assert.throws(() => loadHwpx(fixture(badXml)), /XML/);
    }
  });
  const input = path.join(temp, '원본 # [한글].hwpx'), output = path.join(temp, '작성 결과.hwpx');
  fs.writeFileSync(input, paragraphDoc);
  const req = request(paragraphDoc, [change(paragraphDoc, 'Contents/section0.xml/p0', ['CLI 및 파일 저장 시험'])]);
  test('미리보기는 무저장, 확인값·미리보기 조작·원본 변경 차단', () => {
    const plan = createPlan(input, req, output);
    assert.ok(!fs.existsSync(output));
    assert.throws(() => applyPlan(plan, 'wrong'), /SHA-256/);
    for (const field of ['preview', 'outputPath', 'request']) {
      const altered = clone(plan);
      if (field === 'preview') altered.preview[0].after[0] = '조작';
      if (field === 'outputPath') altered.outputPath = path.join(temp, '조작.hwpx');
      if (field === 'request') altered.request.changes[0].values[0] = '조작';
      assert.throws(() => applyPlan(altered, plan.planSha256), /미리보기/);
    }
    fs.writeFileSync(input, fixture(section(p('바뀐 원본') + p('바꾸지 않을 본문'))));
    assert.throws(() => applyPlan(plan, plan.planSha256), /원본 버전/);
    fs.writeFileSync(input, paragraphDoc);
    assert.ok(!fs.existsSync(output));
  });
  test('저장 부분 실패 후 파일 잔존 없이 재시도', () => {
    const plan = createPlan(input, req, output), originalWrite = fs.writeFileSync;
    fs.writeFileSync = (file, data, options) => {
      if (path.basename(file).startsWith('.tmp-작성 결과.hwpx-')) {
        originalWrite(file, Buffer.from(data).subarray(0, 20), options);
        throw Object.assign(new Error('합성 저장 공간 부족'), { code: 'ENOSPC' });
      }
      return originalWrite(file, data, options);
    };
    try { assert.throws(() => applyPlan(plan, plan.planSha256), /공간 부족/); }
    finally { fs.writeFileSync = originalWrite; }
    assert.ok(!fs.existsSync(output));
    assert.ok(!fs.readdirSync(temp).some(n => n.startsWith('.tmp-')));
    const report = applyPlan(plan, plan.planSha256);
    assert.equal(report.status, 'generated-awaiting-visual');
    assert.deepEqual(fs.readFileSync(input), paragraphDoc);
    assert.throws(() => applyPlan(plan, plan.planSha256), /덮어쓰지/);
    assert.throws(() => createPlan(input, req, output), /이미/);
    assert.equal(validatePlan(plan).report.outputSha256, sha256(fs.readFileSync(output)));
  });
  test('독립 읽기가 다른 내용을 보면 최종 저장 차단', () => {
    const independentOutput = path.join(temp, '독립대조-차단.hwpx');
    const plan = createPlan(input, req, independentOutput), originalWrite = fs.writeFileSync;
    fs.writeFileSync = (file, data, options) => {
      if (path.basename(file) === 'candidate.hwpx' && path.basename(path.dirname(file)).startsWith('hwpx-independent-')) return originalWrite(file, mergedDoc, options);
      return originalWrite(file, data, options);
    };
    try { assert.throws(() => applyPlan(plan, plan.planSha256), /독립|구조가 다릅니다/); }
    finally { fs.writeFileSync = originalWrite; }
    assert.ok(!fs.existsSync(independentOutput));
  });
  test('동시 생성 파일 보존', () => {
    const racePath = path.join(temp, '동시 생성.hwpx'), plan = createPlan(input, req, racePath);
    const originalLink = fs.linkSync;
    fs.linkSync = (from, to) => { fs.writeFileSync(to, '동시 작업 결과'); return originalLink(from, to); };
    try { assert.throws(() => applyPlan(plan, plan.planSha256), { code: 'EEXIST' }); }
    finally { fs.linkSync = originalLink; }
    assert.equal(fs.readFileSync(racePath, 'utf8'), '동시 작업 결과');
  });
  test('작성 후 대상 밖 변경과 입력값 변경 탐지', () => {
    const result = prepareHwpx(paragraphDoc, req), archive = readZipStrict(result.output);
    const modified = patchZip(archive, new Map([['BinData/untouched.bin', Buffer.from('changed')]]));
    assert.throws(() => verifyHwpx(paragraphDoc, req, modified), /산출물/);
    const changedBody = patchZip(archive, new Map([['Contents/section0.xml', Buffer.from(archive.read('Contents/section0.xml').toString().replace('바꾸지 않을 본문', '의도하지 않은 변경'))]]));
    assert.throws(() => verifyHwpx(paragraphDoc, req, changedBody), /산출물/);
  });
  test('실제 Node inspect·preview·apply·verify CLI와 오류 종료', () => {
    const requestPath = path.join(temp, '입력값.json'), planPath = path.join(temp, '미리보기.json'), cliOut = path.join(temp, 'CLI 결과.hwpx');
    fs.writeFileSync(requestPath, JSON.stringify(req));
    const inspect = JSON.parse(success(runNode(formScript, ['inspect', '--input', input])));
    assert.equal(inspect.sourceSha256, sha256(paragraphDoc));
    success(runNode(formScript, ['preview', '--input', input, '--request', requestPath, '--output', cliOut, '--plan', planPath]));
    const plan = JSON.parse(fs.readFileSync(planPath));
    assert.ok(!fs.existsSync(cliOut));
    failure(runNode(formScript, ['apply', '--plan', planPath, '--confirm', 'wrong']));
    success(runNode(formScript, ['apply', '--plan', planPath, '--confirm', plan.planSha256]));
    success(runNode(formScript, ['verify', '--plan', planPath]));
    failure(runNode(formScript, ['inspect', '--input', input, '--unexpected', 'true']));
    failure(runNode(formScript, ['apply', '--plan', planPath, '--confirm', plan.planSha256]));
  });
  if (process.platform === 'win32') {
    const fixtures = [
      ['문단', paragraphDoc], ['병합표', mergedDoc], ['혼합중첩', nestedDoc],
      ['다른접두어', fixture(section(p('공백  유지') + p('둘째<hp:lineBreak/>줄')).replaceAll('hp:', 'x:').replace('xmlns:hp=', 'xmlns:x='))],
      ['빈run', fixture(section('<hp:p><hp:run charPrIDRef="0"/></hp:p>' + p('첫째<hp:tab/>둘째')))],
      ['여러구역', zip([['mimetype', 'application/hwp+zip'], ['Contents/section10.xml', section(p('마지막'))], ['Contents/section2.xml', section(p('앞 구역'))]])],
    ];
    for (const [name, bytes] of fixtures) test(`PowerShell·Node 구조 CLI 독립 대조 ${name}`, () => {
      const file = path.join(temp, `${name}.hwpx`); fs.writeFileSync(file, bytes);
      const psStructure = JSON.parse(success(runPs(path.join(scripts, 'hwpx-structure.ps1'), ['-StructureInputPath', file])).replace(/^\uFEFF/, ''));
      const nodeStructure = loadHwpx(bytes).structure;
      assert.deepEqual(psStructure, nodeStructure);
      assert.deepEqual(psStructure, loadHwpx(bytes).structure);
      assert.deepEqual(fs.readFileSync(file), bytes);
    });
    test('PowerShell 작성 진입점 실제 실행', () => {
      assert.ok(fs.existsSync(formPowerShell), 'PowerShell 작성 진입점 필요');
      const requestPath = path.join(temp, 'PS 입력.json'), planPath = path.join(temp, 'PS 미리보기.json'), psOutput = path.join(temp, 'PS 결과.hwpx');
      fs.writeFileSync(requestPath, JSON.stringify(req));
      const inspected = JSON.parse(success(runPs(formPowerShell, ['-Command', 'inspect', '-InputPath', input])));
      assert.equal(inspected.sourceSha256, sha256(paragraphDoc));
      success(runPs(formPowerShell, ['-Command', 'preview', '-InputPath', input, '-RequestPath', requestPath, '-OutputPath', psOutput, '-PlanPath', planPath]));
      const plan = JSON.parse(fs.readFileSync(planPath));
      success(runPs(formPowerShell, ['-Command', 'apply', '-PlanPath', planPath, '-ConfirmHash', plan.planSha256]));
      success(runPs(formPowerShell, ['-Command', 'verify', '-PlanPath', planPath]));
      const metacharFile = path.join(temp, '특수&ver&rem [값] %PATH% !.hwpx');
      fs.writeFileSync(metacharFile, paragraphDoc);
      const metaResult = JSON.parse(success(runPs(formPowerShell, ['-Command', 'inspect', '-InputPath', metacharFile])));
      assert.equal(metaResult.sourceSha256, sha256(paragraphDoc));
      const missing = runPs(formPowerShell, ['-Command', 'inspect', '-InputPath', 'missing&ver&rem']);
      failure(missing);
      assert.ok(!/Microsoft Windows|\[Version|\[버전/i.test(missing.stdout + missing.stderr), '경로를 셸 명령으로 해석하면 안 됨');
    });
  }
  console.log(`SELFTEST_OK hwpx-form (${cases}개 검사: 공통 문단·표·병합·중첩·입력/원본/미리보기 대조·원본 보존·실패 재시도·실제 CLI${process.platform === 'win32' ? '·PowerShell/Node 독립 구조 대조' : ''}; 화면 검증 별도)`);
} finally {
  // 이 시험에서 만든 OS 임시 폴더만 정리한다.
  const resolved = path.resolve(temp), parent = path.resolve(os.tmpdir());
  if (path.dirname(resolved) === parent && path.basename(resolved).startsWith('hwpx-form-test-')) fs.rmSync(resolved, { recursive: true, force: true });
}
