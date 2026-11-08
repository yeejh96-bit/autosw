import fs from 'node:fs';
import path from 'node:path';
import { createInterface } from 'node:readline/promises';
import { defaultWorkspace, initialize, packageRoot, inside } from './workspace.mjs';
import { setup } from './setup.mjs';

export function npmSetup(workspace, apply = false) {
  const root = path.resolve(workspace || defaultWorkspace());
  if (inside(packageRoot, root) || inside(root, packageRoot)) throw new Error('설치 폴더와 분리된 개인 작업 폴더를 지정하세요.');
  const configPath = path.join(root, '.codex/config.toml'), skillPath = path.join(root, '.agents/skills/autosw-document');
  const preview = { status: 'npm-setup-preview', workspace: root, configPath, skillPath, note: '이 개인 폴더를 준비하고 Codex MCP 연결과 작성 지침을 설치합니다. 버전을 고정한 npx 실행을 등록하며, 첫 실행에는 npm 연결이 필요할 수 있습니다. 전역 설정은 변경하지 않습니다.' };
  if (!apply) return preview;
  for (const target of [configPath, skillPath]) if (fs.existsSync(target)) throw new Error('기존 연결 설정을 보존했습니다. 해당 프로젝트를 열거나 기존 연결을 먼저 확인하세요.');
  initialize(root);
  return setup(root, true, { launcher: 'npm' });
}
export async function setupWizard() {
  const preview = npmSetup();
  if (!process.stdin.isTTY || !process.stdout.isTTY) return { ...preview, note: preview.note + ' 적용하려면 setup --npm --workspace <개인폴더> --apply를 실행하세요.' };
  console.log(`autosw MCP 연결 준비\n개인 작업 폴더: ${preview.workspace}\n설정: ${preview.configPath}\n${preview.note}`);
  const prompt = createInterface({ input: process.stdin, output: process.stdout });
  let answer;
  try { answer = await prompt.question('위 내용으로 연결을 준비할까요? Y를 입력하면 진행합니다: '); }
  finally { prompt.close(); }
  if (answer.trim().toLowerCase() !== 'y') return { status: 'cancelled', note: '변경하지 않았습니다.' };
  const result = npmSetup(preview.workspace, true);
  return { ...result, note: `설정 저장 완료. Codex에서 ${preview.workspace} 폴더를 프로젝트로 열고 신뢰 여부를 직접 확인하세요. 새 대화에서 autosw 환경과 지침을 조회해 실제 앱 연결을 확인하세요.` };
}
