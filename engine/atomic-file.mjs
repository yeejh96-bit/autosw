// 완성된 임시 파일을 공개한다. new는 동시 생성된 대상도 덮어쓰지 않는다.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

export function writeAtomic(filePath, content, { mode = 'overwrite' } = {}) {
  if (!['new', 'overwrite'].includes(mode)) throw new Error(`잘못된 저장 모드: ${mode}`);
  const temporary = path.join(path.dirname(filePath), `.tmp-${path.basename(filePath)}-${crypto.randomUUID()}`);
  try {
    fs.writeFileSync(temporary, content, { encoding: 'utf8', flag: 'wx' });
    if (mode === 'new') {
      // 같은 디렉터리의 완성된 파일을 연결한다. 대상이 있으면 EEXIST로 실패한다.
      fs.linkSync(temporary, filePath);
    } else {
      fs.renameSync(temporary, filePath);
    }
  } finally {
    try {
      fs.rmSync(temporary, { force: true });
    } catch (error) {
      // 공개가 끝난 저장을 실패로 보고해 재시도시키지 않는다.
      console.error(`[경고] 저장 임시 파일 정리 실패: ${temporary} (${error.message})`);
    }
  }
}
