// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
import { format } from "util";
export const VERSION = typeof __KORDOC_VERSION__ !== "undefined" ? __KORDOC_VERSION__ : "0.0.0-dev";
export function toArrayBuffer(buf) {
    if (buf.byteOffset === 0 && buf.byteLength === buf.buffer.byteLength) {
        return buf.buffer;
    }
    return buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength);
}
export class KordocError extends Error {
    constructor(message){
        super(message);
        this.name = "KordocError";
    }
}
function isFileNotFound(err) {
    if (typeof err !== "object" || err === null) return false;
    if (err.code === "ENOENT") return true;
    const msg = err.message;
    return typeof msg === "string" && /ENOENT|no such file or directory/i.test(msg);
}
export function sanitizeError(err) {
    if (err instanceof KordocError) return err.message;
    if (isFileNotFound(err)) return "파일 또는 디렉토리를 찾을 수 없습니다";
    return "문서 처리 중 오류가 발생했습니다";
}
export function isPathTraversal(name) {
    if (name.includes("\x00")) return true;
    const normalized = name.replace(/\\/g, "/");
    const segments = normalized.split("/");
    return segments.some((s)=>s === "..") || normalized.startsWith("/") || /^[A-Za-z]:/.test(normalized);
}
export function normalizeSectionHref(href) {
    if (!href) return null;
    let normalized = href.replace(/\\/g, "/").replace(/^\/+/, "");
    if (isPathTraversal(normalized)) return null;
    if (/^[Ss]ection\d+\.xml$/.test(normalized)) normalized = "Contents/" + normalized;
    return /(?:^|\/)[Ss]ection\d+\.xml$/.test(normalized) ? normalized : null;
}
export function compareSectionPaths(a, b) {
    const ai = Number(a.match(/[Ss]ection(\d+)\.xml$/)?.[1] ?? Number.MAX_SAFE_INTEGER);
    const bi = Number(b.match(/[Ss]ection(\d+)\.xml$/)?.[1] ?? Number.MAX_SAFE_INTEGER);
    return ai === bi ? a.localeCompare(b) : ai - bi;
}
const MAX_UNZIP_ENV_MB = 8192;
export function unzipLimitBytes(defaultBytes) {
    const mb = Number(process.env.KORDOC_MAX_UNZIP_MB);
    return Number.isFinite(mb) && mb > 0 ? Math.min(mb, MAX_UNZIP_ENV_MB) * 1024 * 1024 : defaultBytes;
}
export function precheckZipSize(buffer, maxUncompressedSize = 256 * 1024 * 1024, maxEntries = 500, media) {
    try {
        const data = new DataView(buffer);
        const len = buffer.byteLength;
        let eocdOffset = -1;
        for(let i = len - 22; i >= Math.max(0, len - 65557); i--){
            if (data.getUint32(i, true) === 0x06054b50) {
                eocdOffset = i;
                break;
            }
        }
        if (eocdOffset < 0) return {
            totalUncompressed: 0,
            entryCount: 0
        };
        const entryCount = data.getUint16(eocdOffset + 10, true);
        if (entryCount > maxEntries) {
            throw new KordocError(`ZIP 엔트리 수 초과: ${entryCount} (최대 ${maxEntries})`);
        }
        const cdSize = data.getUint32(eocdOffset + 12, true);
        const cdOffset = data.getUint32(eocdOffset + 16, true);
        if (cdOffset + cdSize > len) return {
            totalUncompressed: 0,
            entryCount
        };
        let totalUncompressed = 0;
        let mediaUncompressed = 0;
        let pos = cdOffset;
        for(let i = 0; i < entryCount && pos + 46 <= cdOffset + cdSize; i++){
            if (data.getUint32(pos, true) !== 0x02014b50) break;
            const localOff = data.getUint32(pos + 42, true);
            const localSize = localOff + 30 <= len && data.getUint32(localOff, true) === 0x04034b50 ? data.getUint32(localOff + 22, true) : 0;
            const size = Math.max(data.getUint32(pos + 24, true), localSize);
            const nameLen = data.getUint16(pos + 28, true);
            const extraLen = data.getUint16(pos + 30, true);
            const commentLen = data.getUint16(pos + 32, true);
            const isMedia = !!media && pos + 46 + nameLen <= len && media.re.test(new TextDecoder().decode(new Uint8Array(buffer, pos + 46, nameLen)));
            const never = isMedia && !!media?.never && media.never.test(new TextDecoder().decode(new Uint8Array(buffer, pos + 46, nameLen)));
            if (isMedia && !never) mediaUncompressed += size;
            if (!(isMedia && (media?.skip || never))) totalUncompressed += size;
            pos += 46 + nameLen + extraLen + commentLen;
        }
        if (totalUncompressed > maxUncompressedSize) {
            const mb = (n)=>(n / 1024 / 1024).toFixed(1);
            const mediaNote = media && !media.skip && mediaUncompressed > 0 ? ` — 그림·개체 파트가 ${mb(mediaUncompressed)}MB, 이미지 추출을 끄면(images: false·--no-images) 세지 않는다` : "";
            throw new KordocError(`ZIP 비압축 크기 초과: ${mb(totalUncompressed)}MB (최대 ${maxUncompressedSize / 1024 / 1024}MB)${mediaNote}`);
        }
        return {
            totalUncompressed,
            entryCount
        };
    } catch (err) {
        if (err instanceof KordocError) throw err;
        return {
            totalUncompressed: 0,
            entryCount: 0
        };
    }
}
export function stripDtd(xml) {
    return xml.replace(/^\uFEFF/, "").replace(/<!DOCTYPE\s[^[>]*(\[[\s\S]*?\])?\s*>/gi, "");
}
const SAFE_HREF_RE = /^(?:https?:|mailto:|tel:|#)/i;
const HREF_ESCAPE = {
    "(": "%28",
    ")": "%29",
    "<": "%3C",
    ">": "%3E",
    "\"": "%22",
    "'": "%27",
    "`": "%60"
};
export function sanitizeHref(href) {
    const trimmed = href.trim();
    if (!trimmed || !SAFE_HREF_RE.test(trimmed)) return null;
    return trimmed.replace(/[()<>"'`]/g, (c)=>HREF_ESCAPE[c]).replace(/\s/g, (c)=>encodeURIComponent(c));
}
export function escapeHtml(text, attr = false) {
    const s = text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    return attr ? s.replace(/"/g, "&quot;") : s;
}
export function unescapeHtml(text) {
    return text.replace(/&(lt|gt|quot|#39|amp);/g, (_, e)=>HTML_ENTITY[e]);
}
const HTML_ENTITY = {
    lt: "<",
    gt: ">",
    quot: "\"",
    "#39": "'",
    amp: "&"
};
export function routeConsoleToStderr() {
    const toStderr = (...args)=>void process.stderr.write(format(...args) + "\n");
    console.log = toStderr;
    console.info = toStderr;
    console.warn = toStderr;
    console.debug = toStderr;
}
export function safeMin(arr) {
    let min = Infinity;
    for(let i = 0; i < arr.length; i++)if (arr[i] < min) min = arr[i];
    return min;
}
export function safeMax(arr) {
    let max = -Infinity;
    for(let i = 0; i < arr.length; i++)if (arr[i] > max) max = arr[i];
    return max;
}
export const OPTIONAL_DEP_INSTALL_HINT = " 네트워크가 제한된 linux/x64 에서 npx 로 설치했다면 onnxruntime-node 의 CUDA 다운로드 실패로 함께 빠졌을 수 있습니다 — " + "`ONNXRUNTIME_NODE_INSTALL=skip npx -y kordoc@^4 …` 로 다시 설치하세요.";
export function classifyError(err) {
    if (!(err instanceof Error)) return "PARSE_ERROR";
    const msg = err.message;
    if (isFileNotFound(err)) return "FILE_NOT_FOUND";
    if (msg.includes("DRM")) return "DRM_PROTECTED";
    if (msg.includes("암호화") || msg.includes("암호로 보호")) return "ENCRYPTED";
    if (msg.includes("optional dependency")) return "MISSING_DEPENDENCY";
    if (msg.includes("Invalid string length") || msg.includes("Cannot create a string longer")) return "OUTPUT_TOO_LARGE";
    if (msg.includes("ZIP bomb") || msg.includes("ZIP 비압축 크기 초과") || msg.includes("ZIP 엔트리 수 초과")) return "ZIP_BOMB";
    if (msg.includes("bomb") || msg.includes("크기 초과") || msg.includes("압축 해제")) return "DECOMPRESSION_BOMB";
    if (msg.includes("이미지 기반")) return "IMAGE_BASED_PDF";
    if (msg.includes("섹션") && (msg.includes("찾을 수 없") || msg.includes("없음"))) return "NO_SECTIONS";
    if (msg.includes("시그니처") || msg.includes("복구할 수 없")) return "CORRUPTED";
    return "PARSE_ERROR";
}
export function partExtension(path) {
    return (/\.([A-Za-z0-9]{1,5})$/.exec(path.slice(path.lastIndexOf("/") + 1))?.[1] ?? "bin").toLowerCase();
}
