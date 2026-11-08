import {loadHwpx,sha256} from './hwpx-document.mjs';
import {parseOfficeXml} from './office-zip.mjs';
import {previewFonts} from './hwpx-preview-fonts.mjs';

// 복잡한 기존 양식용. 파일 직접 읽기만 사용하며 COM·외부 URL·OCR를 호출하지 않는다.
export async function renderHwpxLayout(bytes){
  loadHwpx(bytes); // 기존 ZIP/XML·암호화·스크립트 검사 유지
  const {renderHwpxToSvg}=await import('../vendor/hwpx-layout/render/svg-render.mjs');
  const result=await renderHwpxToSvg(bytes,{reflow:true});
  if(!result.pageCount||!result.svg)throw new Error('배치 미리보기 본문 없음');
  validateRenderedSvg(result.svg);
  const fonts=previewFonts(result.svg);
  return {fontCss:fonts.css,svgs:[result.svg],report:{schemaVersion:1,sourceSha256:sha256(bytes),status:'approximate-preview',engine:'layout',engineVersion:'local-2',embeddedFonts:fonts.fonts,pageCount:result.pageCount,stats:result.stats,warnings:['파일에 저장된 배치와 자체 조판을 사용한 미리보기입니다. 실제 한글 출력과 다를 수 있습니다. 한글 전용 글꼴은 대응하는 Windows 글꼴로 표시하며 화면의 글자 가장자리는 한글 출력과 다를 수 있습니다.',...result.warnings,...fonts.warnings],hangulVerified:false}};
}
export function validateRenderedSvg(svg){
  const parsed=parseOfficeXml(svg);
  for(const n of parsed.nodes){
    if(['script','foreignObject','iframe','style','animate','set'].includes(n.local))throw new Error('미리보기의 실행 요소 차단');
    for(const [key,value] of Object.entries(n.attrs)){
      if(/^on/i.test(key))throw new Error('미리보기 이벤트 차단');
      if(/(?:^|:)href$/.test(key)&&!/^#/.test(value)&&!/^data:image\/(?:png|jpeg|gif|webp);base64,[A-Za-z0-9+/=\s]+$/.test(value))throw new Error('미리보기 외부 참조 차단');
      if(/url\(/i.test(value)&&!/^url\(#[\w-]+\)$/.test(value))throw new Error('미리보기 외부 스타일 참조 차단');
    }
  }
}
