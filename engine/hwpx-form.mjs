// 공통 HWPX 작성 진입점. 원본/미리보기/입력값을 묶고 새 파일만 공개한다.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { fileURLToPath } from 'node:url';
import { inspectHwpx, loadHwpx, prepareHwpx, verifyHwpx, sha256 } from './hwpx-document.mjs';
import { writeAtomic } from './atomic-file.mjs';

const digest = obj => sha256(Buffer.from(JSON.stringify(obj)));
export function validateIndependent(bytes) {
  if (process.platform !== 'win32') throw new Error('작성 결과의 독립 대조는 Windows PowerShell이 필요합니다.');
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'hwpx-independent-'));
  const candidate = path.join(directory, 'candidate.hwpx');
  try {
    fs.writeFileSync(candidate, bytes, { flag: 'wx' });
    const helper = fileURLToPath(new URL('./hwpx-structure.ps1', import.meta.url));
    const result = spawnSync('powershell.exe', ['-NoProfile', '-File', helper, '-StructureInputPath', candidate], { encoding: 'utf8', windowsHide: true, timeout: 60000, maxBuffer: 64 * 1024 * 1024 });
    if (result.error || result.status !== 0) throw new Error(`PowerShell 독립 읽기 실패: ${result.error?.message ?? result.stderr.trim()}`);
    const independent = JSON.parse(result.stdout.replace(/^\uFEFF/, ''));
    if (!isDeepStrictEqual(independent, loadHwpx(bytes).structure)) throw new Error('PowerShell·Node의 작성 결과 구조가 다릅니다.');
  } finally {
    // 정확히 이번 검사용 파일과 빈 폴더만 정리한다. 기존 사용자 파일에는 접근하지 않는다.
    if (fs.existsSync(candidate)) fs.unlinkSync(candidate);
    fs.rmdirSync(directory);
  }
}
const readJson = file => {
  if (fs.statSync(file).size > 16 * 1024 * 1024) throw new Error('입력 JSON이 너무 큽니다.');
  return JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
};
function readSource(file) {
  const resolved = fs.realpathSync(file), stat = fs.statSync(resolved);
  if (!stat.isFile() || stat.size > 128 * 1024 * 1024) throw new Error('지원하지 않는 원본 크기·종류입니다.');
  return { sourcePath: resolved, bytes: fs.readFileSync(resolved) };
}
function destination(file) {
  const out = path.resolve(file);
  if (path.extname(out).toLowerCase() !== '.hwpx') throw new Error('새 산출물은 .hwpx로 지정하세요.');
  if (!fs.statSync(path.dirname(out)).isDirectory()) throw new Error('출력 폴더가 없습니다.');
  return out;
}
export function createPlan(input, request, output) {
  const { sourcePath, bytes } = readSource(input), outputPath = destination(output);
  if (fs.existsSync(outputPath)) throw new Error('출력 파일이 이미 있습니다. 새 이름을 지정하세요.');
  const prepared = prepareHwpx(bytes, request);
  const body = { schemaVersion: 1, sourcePath, outputPath, request, preview: prepared.preview, unassigned: prepared.unassigned, expectedOutputSha256: prepared.report.outputSha256, visualStatus: 'not-checked' };
  return { ...body, planSha256: digest(body) };
}
export function validatePlan(plan) {
  if (!plan || typeof plan !== 'object' || Array.isArray(plan)) throw new Error('잘못된 미리보기입니다.');
  const { planSha256, ...body } = plan;
  if (digest(body) !== planSha256) throw new Error('미리보기 내용이 변경됐습니다.');
  const source = readSource(plan.sourcePath);
  if (source.sourcePath !== plan.sourcePath || path.resolve(plan.outputPath) !== plan.outputPath) throw new Error('미리보기 경로가 변경됐습니다.');
  const prepared = prepareHwpx(source.bytes, plan.request);
  const expected = { schemaVersion: 1, sourcePath: source.sourcePath, outputPath: destination(plan.outputPath), request: plan.request, preview: prepared.preview, unassigned: prepared.unassigned, expectedOutputSha256: prepared.report.outputSha256, visualStatus: 'not-checked' };
  if (digest(expected) !== planSha256) throw new Error('미리보기와 현재 원본·입력·출력 경로가 다릅니다.');
  return { ...prepared, source };
}
export function applyPlan(plan, confirmation) {
  if (confirmation !== plan.planSha256) throw new Error('확인한 미리보기의 SHA-256을 지정하세요.');
  const prepared = validatePlan(plan);
  if (fs.existsSync(plan.outputPath)) throw new Error('기존 산출물을 덮어쓰지 않습니다. verify로 기존 파일을 확인하세요.');
  validateIndependent(prepared.output);
  // 생성 직전 원본 재확인. 원본의 파일 핸들에 쓰는 경로는 없다.
  if (sha256(readSource(plan.sourcePath).bytes) !== plan.request.sourceSha256) throw new Error('원본이 미리보기 이후 변경됐습니다.');
  writeAtomic(plan.outputPath, prepared.output, { mode: 'new' });
  const report = verifyHwpx(prepared.source.bytes, plan.request, fs.readFileSync(plan.outputPath));
  return { ...report, independentCheck: 'passed', outputPath: plan.outputPath, status: 'generated-awaiting-visual' };
}
export function main(args = process.argv.slice(2)) {
  const command = args.shift();
  if (command === '--help' || !command) {
    console.log('inspect --input HWPX [--output JSON]\npreview --input HWPX --request JSON --output 새HWPX --plan 새JSON\napply --plan JSON --confirm 미리보기SHA256\nverify --plan JSON'); return;
  }
  const allowed = { inspect: ['input', 'output'], preview: ['input', 'request', 'output', 'plan'], apply: ['plan', 'confirm'], verify: ['plan'] }[command];
  if (!allowed) throw new Error('알 수 없는 명령입니다. --help를 확인하세요.');
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i].slice(2);
    if (!args[i].startsWith('--') || !allowed.includes(key) || Object.hasOwn(options, key) || !args[i + 1] || args[i + 1].startsWith('--')) throw new Error('잘못되거나 중복된 인자입니다.');
    options[key] = args[i + 1];
  }
  const required = command === 'inspect' ? ['input'] : allowed;
  if (required.some(k => !options[k])) throw new Error('필수 인자가 없습니다. --help를 확인하세요.');
  if (command === 'inspect') {
    const result = inspectHwpx(readSource(options.input).bytes), json = JSON.stringify(result, null, 2) + '\n';
    if (options.output) { writeAtomic(path.resolve(options.output), json, { mode: 'new' }); console.log(`구조 저장: ${path.resolve(options.output)}`); }
    else console.log(json);
  } else if (command === 'preview') {
    const plan = createPlan(options.input, readJson(options.request), options.output);
    if (path.resolve(options.plan).toLowerCase() === plan.outputPath.toLowerCase()) throw new Error('미리보기와 산출물 경로를 분리하세요.');
    writeAtomic(path.resolve(options.plan), JSON.stringify(plan, null, 2) + '\n', { mode: 'new' });
    console.log(JSON.stringify({ planPath: path.resolve(options.plan), ...plan }, null, 2));
  } else if (command === 'apply') {
    console.log(JSON.stringify(applyPlan(readJson(options.plan), options.confirm), null, 2));
  } else {
    const plan = readJson(options.plan), prepared = validatePlan(plan);
    const actual = fs.readFileSync(plan.outputPath);
    const report = verifyHwpx(prepared.source.bytes, plan.request, actual);
    validateIndependent(actual);
    console.log(JSON.stringify({ ...report, independentCheck: 'passed', outputPath: plan.outputPath, status: 'generated-awaiting-visual' }, null, 2));
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(`실패: ${error.message}`); process.exitCode = 1; }
}
