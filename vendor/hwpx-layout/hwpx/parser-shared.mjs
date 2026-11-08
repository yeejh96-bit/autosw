// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
import { DOMParser } from "@xmldom/xmldom";
import { KordocError, unzipLimitBytes } from "../utils.mjs";
export const MAX_DECOMPRESS_SIZE = unzipLimitBytes(256 * 1024 * 1024);
export const MAX_ZIP_ENTRIES = 500;
export class ZipBombError extends KordocError {
    constructor(message){
        super(message);
        this.name = "ZipBombError";
    }
}
export function readZipEntry(file, type, used = 0) {
    return new Promise((resolve, reject)=>{
        const chunks = [];
        let size = 0;
        const declared = file._data?.uncompressedSize;
        const limit = typeof declared === "number" && declared >= 0 ? declared : Number.POSITIVE_INFINITY;
        const stream = file.nodeStream("nodebuffer");
        stream.on("data", (chunk)=>{
            size += chunk.length;
            if (used + size > MAX_DECOMPRESS_SIZE || size > limit) {
                stream.pause();
                reject(new ZipBombError("ZIP 압축 해제 크기 초과 (ZIP bomb 의심)"));
                return;
            }
            chunks.push(chunk);
        });
        stream.on("error", reject);
        stream.on("end", ()=>{
            if (type === "text") return resolve(Buffer.concat(chunks, size).toString("utf-8"));
            const out = new Uint8Array(size);
            let at = 0;
            for (const c of chunks){
                out.set(c, at);
                at += c.length;
            }
            resolve(out);
        });
    });
}
export function clampSpan(val, max) {
    return Math.max(1, Math.min(val, max));
}
export { MAX_XML_DEPTH, findChildByLocalName, extractTextFromNode } from "../shared/xml.mjs";
export function createSectionShared() {
    return {
        numState: new Map(),
        pageText: {
            headers: [],
            footers: []
        },
        track: {
            deleteDepth: 0,
            warned: false,
            deletedObjects: new WeakSet(),
            deletedImageRefs: new Set()
        },
        pageState: {
            base: 0,
            allUsable: true
        },
        tocTexts: new Set(),
        memos: {
            list: [],
            seen: new WeakSet()
        }
    };
}
export function createXmlParser(warnings) {
    return new DOMParser({
        onError (level, msg) {
            if (level === "fatalError") throw new KordocError(`XML 파싱 실패: ${msg}`);
            warnings?.push({
                code: "MALFORMED_XML",
                message: `XML ${level === "warning" ? "경고" : "오류"}: ${msg}`
            });
        }
    });
}
export function applyPageText(blocks, shared) {
    const { headers, footers } = shared.pageText;
    if (headers.length > 0) {
        blocks.unshift(...headers.map((t)=>({
                type: "paragraph",
                text: t,
                pageNumber: 1
            })));
    }
    if (footers.length > 0) {
        blocks.push(...footers.map((t)=>({
                type: "paragraph",
                text: t
            })));
    }
}
