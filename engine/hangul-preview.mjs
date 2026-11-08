import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {writeAtomic} from './atomic-file.mjs';
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
export function bindPreview(source,pdf,expectedSourceHash,pageCount){
  const sourceSha256=hash(source);
  if(sourceSha256.toLowerCase()!==expectedSourceHash.toLowerCase())throw new Error('원본 변경: 미리보기를 다시 생성해야 합니다.');
  const pdfBytes=fs.readFileSync(pdf);
  if(!pdfBytes.subarray(0,5).equals(Buffer.from('%PDF-')))throw new Error('PDF 형식 오류');
  if(!Number.isSafeInteger(pageCount)||pageCount<1)throw new Error('쪽 수 오류');
  return {schemaVersion:1,status:'hancom-output-preview',sourcePath:fs.realpathSync(source),sourceSha256,pdfPath:fs.realpathSync(pdf),pdfSha256:crypto.createHash('sha256').update(pdfBytes).digest('hex'),pageCount,visualStatus:'awaiting-review'};
}
export function verifyPreview(report){
  if(report.schemaVersion!==1||report.status!=='hancom-output-preview')throw new Error('미리보기 기록 형식 오류');
  if(hash(report.sourcePath)!==report.sourceSha256)throw new Error('원본 변경: 미리보기를 다시 생성해야 합니다.');
  if(hash(report.pdfPath)!==report.pdfSha256)throw new Error('PDF 변경: 미리보기를 다시 생성해야 합니다.');
  return {status:'current-hancom-output',pdfPath:report.pdfPath,pageCount:report.pageCount,visualStatus:report.visualStatus};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  try{const [command,...args]=process.argv.slice(2);let result;
    if(command==='bind'&&args.length===5){const [source,pdf,expected,pages,report]=args;result=bindPreview(source,pdf,expected,Number(pages));writeAtomic(report,JSON.stringify(result,null,2)+'\n',{mode:'new'});}
    else if(command==='verify'&&args.length===1)result=verifyPreview(JSON.parse(fs.readFileSync(args[0],'utf8').replace(/^\uFEFF/,'')));
    else throw new Error('bind SOURCE PDF SOURCE_HASH PAGES REPORT 또는 verify REPORT');
    console.log(JSON.stringify(result,null,2));
  }catch(e){console.error(e.message);process.exitCode=1;}
}
