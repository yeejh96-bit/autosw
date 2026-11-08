import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {composeHwpx,normalizeDocument,documentFromMarkdown} from './hwpx-compose.mjs';
import {renderHwpx,previewHtml} from './hwpx-render.mjs';
import {renderHwpxLayout} from './hwpx-render-layout.mjs';
import {loadHwpx,sha256} from './hwpx-document.mjs';
import {writeAtomic} from './atomic-file.mjs';
import {validateIndependent} from './hwpx-form.mjs';
const digest=o=>sha256(Buffer.from(JSON.stringify(o)));
function source(file){const real=fs.realpathSync(file),s=fs.statSync(real);if(!s.isFile()||s.size>4*1024*1024)throw new Error('입력은 4MB 이하 파일이어야 합니다.');return {path:real,bytes:fs.readFileSync(real)};}
const decode=bytes=>new TextDecoder('utf-8',{fatal:true}).decode(bytes).replace(/^\uFEFF/,'');
function modelFor(src){const text=decode(src.bytes);if(path.extname(src.path).toLowerCase()==='.md')return documentFromMarkdown(text);if(path.extname(src.path).toLowerCase()!=='.json')throw new Error('생성 입력은 .json 또는 .md');return normalizeDocument(JSON.parse(text));}
function destination(file,extension){const out=path.resolve(file);if(path.extname(out).toLowerCase()!==extension)throw new Error(`출력 확장자는 ${extension}`);if(!fs.statSync(path.dirname(out)).isDirectory())throw new Error('출력 폴더 없음');return out;}
function absent(file){if(fs.existsSync(file))throw new Error(`기존 파일은 덮어쓰지 않습니다: ${file}`);}
export async function previewCreation(input,output){
  const src=source(input),out=destination(output,'.hwpx');absent(out);
  const generated=composeHwpx(modelFor(src)),render=await renderHwpx(generated.bytes);
  const body={schemaVersion:1,sourcePath:src.path,sourceSha256:sha256(src.bytes),outputPath:out,model:generated.model,expectedOutputSha256:sha256(generated.bytes),preview:render.report};
  return {...body,planSha256:digest(body)};
}
export function validateCreation(plan){
  if(!plan||typeof plan!=='object'||Array.isArray(plan))throw new Error('잘못된 생성 미리보기');
  const {planSha256,...body}=plan;if(plan.schemaVersion!==1||digest(body)!==planSha256)throw new Error('미리보기 내용 변경');
  const src=source(plan.sourcePath);if(src.path!==plan.sourcePath||sha256(src.bytes)!==plan.sourceSha256)throw new Error('원고 변경 — 새 미리보기가 필요합니다.');
  if(destination(plan.outputPath,'.hwpx')!==plan.outputPath)throw new Error('출력 경로 변경');
  const generated=composeHwpx(modelFor(src));
  if(digest(generated.model)!==digest(plan.model)||sha256(generated.bytes)!==plan.expectedOutputSha256)throw new Error('생성 규칙·원고·미리보기 불일치');
  return generated;
}
export function applyCreation(plan,confirmation){
  if(confirmation!==plan?.planSha256)throw new Error('확인한 미리보기 SHA-256이 필요합니다.');
  const generated=validateCreation(plan);absent(plan.outputPath);validateIndependent(generated.bytes);
  if(sha256(source(plan.sourcePath).bytes)!==plan.sourceSha256)throw new Error('저장 전 원고 변경');
  writeAtomic(plan.outputPath,generated.bytes,{mode:'new'});
  return verifyCreation(plan);
}
export function verifyCreation(plan){
  validateCreation(plan);const actual=fs.readFileSync(plan.outputPath);
  if(sha256(actual)!==plan.expectedOutputSha256)throw new Error('생성본이 변경됐습니다. 한글 재저장본이면 별도 구조·화면 대조가 필요합니다.');
  validateIndependent(actual);const doc=loadHwpx(actual);
  return {status:'generated-awaiting-hangul',outputPath:plan.outputPath,sha256:sha256(actual),independentCheck:'passed',paragraphs:doc.structure.sections.flatMap(s=>s.paragraphs).length,tables:doc.structure.sections.flatMap(s=>s.tables).length};
}
export async function renderFile(input,output,reportPath,engine='native'){
  if(!['native','layout'].includes(engine))throw new Error('미리보기 엔진은 native 또는 layout');
  const real=fs.realpathSync(input);if(fs.statSync(real).size>128*1024*1024)throw new Error('입력 크기 초과');
  const out=destination(output,'.html'),report=reportPath?destination(reportPath,'.json'):null;absent(out);if(report)absent(report);
  const bytes=fs.readFileSync(real),render=await (engine==='layout'?renderHwpxLayout(bytes):renderHwpx(bytes));
  if(sha256(fs.readFileSync(real))!==render.report.sourceSha256)throw new Error('렌더 중 입력 파일 변경');
  writeAtomic(out,previewHtml(render),{mode:'new'});if(report)writeAtomic(report,JSON.stringify(render.report,null,2)+'\n',{mode:'new'});
  return {...render.report,outputPath:out,...(report?{reportPath:report}:{})};
}
export async function main(args=process.argv.slice(2)){
  const command=args.shift();if(!command||command==='--help'){console.log('preview --input 원고.json|md --output 새.hwpx --plan 새.json\napply --plan JSON --confirm SHA256\nverify --plan JSON\nrender --input HWPX --output 새.html [--report 새.json]');return;}
  const allowed={preview:['input','output','plan'],apply:['plan','confirm'],verify:['plan'],render:['input','output','report','engine']}[command];if(!allowed)throw new Error('알 수 없는 명령');
  const opt={};for(let i=0;i<args.length;i+=2){const k=args[i].slice(2);if(!args[i].startsWith('--')||!allowed.includes(k)||Object.hasOwn(opt,k)||!args[i+1]||args[i+1].startsWith('--'))throw new Error('잘못되거나 중복된 인자');opt[k]=args[i+1];}
  if(allowed.filter(k=>!(command==='render'&&['report','engine'].includes(k))).some(k=>!opt[k]))throw new Error('필수 인자 없음');
  const readPlan=()=>JSON.parse(decode(source(opt.plan).bytes));
  let result;
  if(command==='preview'){const out=destination(opt.plan,'.json');absent(out);const plan=await previewCreation(opt.input,opt.output);writeAtomic(out,JSON.stringify(plan,null,2)+'\n',{mode:'new'});result={planPath:out,planSha256:plan.planSha256,model:plan.model,preview:plan.preview};}
  else if(command==='apply')result=applyCreation(readPlan(),opt.confirm);
  else if(command==='verify')result=verifyCreation(readPlan());
  else result=await renderFile(opt.input,opt.output,opt.report,opt.engine);
  console.log(JSON.stringify(result,null,2));
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){try{await main();}catch(e){console.error(`실패: ${e.message}`);process.exitCode=1;}}
