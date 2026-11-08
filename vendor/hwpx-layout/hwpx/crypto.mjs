// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
import { createDecipheriv, createHash, pbkdf2Sync, timingSafeEqual } from "crypto";
import { inflateRawSync } from "zlib";
import { KordocError } from "../utils.mjs";
import { MAX_DECOMPRESS_SIZE, readZipEntry, ZipBombError } from "./parser-shared.mjs";
const MAX_ITERATION_COUNT = 1_000_000;
const AES256_CBC = "http://www.w3.org/2001/04/xmlenc#aes256-cbc";
const START_KEY_SHA256 = "http://www.w3.org/2000/09/xmldsig#sha256";
const CHECKSUM_SHA256_1K = "sha256-1k";
const CHECKSUM_PREFIX_BYTES = 1024;
export function isEncryptedManifest(manifestXml) {
    return manifestXml.includes("encryption-data");
}
export async function lockedHwpxError(zip) {
    const hpfFile = zip.file("Contents/content.hpf");
    const hpf = hpfFile ? await readZipEntry(hpfFile, "text").catch(()=>"") : "";
    return new KordocError(/\bdistribution="1"/.test(hpf ?? "") ? "배포용으로 암호화된 HWPX 파일입니다 (배포용 복호는 지원하지 않음). 한컴오피스에서 일반 문서로 저장한 뒤 여세요." : "암호로 보호된 HWPX 파일입니다. password 옵션에 열기 암호를 지정하세요.");
}
const attrOf = (xml, name)=>xml.match(new RegExp(`${name}="([^"]*)"`))?.[1];
export function parseEncryptionManifest(manifestXml) {
    const entries = [];
    const entryRe = /<[^:>\s]*:?file-entry\b([^>]*)>([\s\S]*?)<\/[^:>\s]*:?file-entry>/g;
    let m;
    while((m = entryRe.exec(manifestXml)) !== null){
        const [, attrs, inner] = m;
        if (!inner.includes("encryption-data")) continue;
        const path = attrOf(attrs, "full-path");
        if (!path) continue;
        const algorithm = attrOf(inner, "algorithm-name");
        const startKey = attrOf(inner, "start-key-generation-name");
        const checksumType = attrOf(inner, "checksum-type");
        if (algorithm !== AES256_CBC) {
            throw new KordocError(`지원하지 않는 HWPX 암호화 방식입니다: ${algorithm ?? "(미상)"}`);
        }
        if (startKey !== START_KEY_SHA256) {
            throw new KordocError(`지원하지 않는 HWPX 시작키 생성 방식입니다: ${startKey ?? "(미상)"}`);
        }
        if (!checksumType?.endsWith(CHECKSUM_SHA256_1K)) {
            throw new KordocError(`지원하지 않는 HWPX 체크섬 방식입니다: ${checksumType ?? "(미상)"}`);
        }
        const iterations = parseInt(attrOf(inner, "iteration-count") ?? "", 10);
        const keySize = parseInt(attrOf(inner, "key-size") ?? "", 10);
        const iv = Buffer.from(attrOf(inner, "initialisation-vector") ?? "", "base64");
        const salt = Buffer.from(attrOf(inner, "salt") ?? "", "base64");
        const checksum = Buffer.from(attrOf(inner, "checksum") ?? "", "base64");
        if (!(iterations > 0 && iterations <= MAX_ITERATION_COUNT)) {
            throw new KordocError(`HWPX 암호화 반복 횟수가 허용 범위를 벗어났습니다: ${iterations}`);
        }
        if (keySize !== 32 || iv.length !== 16 || salt.length === 0 || checksum.length !== 32) {
            throw new KordocError(`HWPX 암호화 파라미터가 올바르지 않습니다 (${path})`);
        }
        entries.push({
            path,
            checksum,
            iv,
            salt,
            iterations,
            keySize
        });
    }
    return entries;
}
export function stripEncryptionData(manifestXml) {
    return manifestXml.replace(/<([^:>\s]*:?)encryption-data\b[\s\S]*?<\/\1encryption-data>/g, "");
}
function decryptEntry(cipher, entry, startKey) {
    if (cipher.length === 0 || cipher.length % 16 !== 0) return undefined;
    for (const prf of [
        "sha1",
        "sha256"
    ]){
        try {
            const key = pbkdf2Sync(startKey, entry.salt, entry.iterations, entry.keySize, prf);
            const decipher = createDecipheriv("aes-256-cbc", key, entry.iv);
            decipher.setAutoPadding(false);
            const raw = Buffer.concat([
                decipher.update(cipher),
                decipher.final()
            ]);
            const plain = inflateRawSync(raw, {
                maxOutputLength: MAX_DECOMPRESS_SIZE
            });
            const sum = createHash("sha256").update(plain.subarray(0, Math.min(CHECKSUM_PREFIX_BYTES, plain.length))).digest();
            if (sum.length === entry.checksum.length && timingSafeEqual(sum, entry.checksum)) return plain;
        } catch  {}
    }
    return undefined;
}
export async function decryptHwpxInPlace(zip, manifestXml, password) {
    const entries = parseEncryptionManifest(manifestXml);
    if (entries.length === 0) return 0;
    const startKey = createHash("sha256").update(Buffer.from(password, "utf8")).digest();
    const decrypted = [];
    let used = 0;
    for (const entry of entries){
        const file = zip.file(entry.path);
        if (!file) {
            throw new KordocError(`암호 목록에 있는 파일이 문서에 없습니다: ${entry.path}`);
        }
        const raw = await readZipEntry(file, "uint8array", used);
        const plain = decryptEntry(Buffer.from(raw.buffer, raw.byteOffset, raw.length), entry, startKey);
        if (!plain) {
            throw new KordocError("비밀번호가 일치하지 않거나 암호화 데이터가 손상되었습니다.");
        }
        used += plain.length;
        if (used > MAX_DECOMPRESS_SIZE) throw new ZipBombError("ZIP 압축 해제 크기 초과 (ZIP bomb 의심)");
        decrypted.push({
            path: entry.path,
            plain
        });
    }
    for (const { path, plain } of decrypted)zip.file(path, plain);
    zip.file("META-INF/manifest.xml", stripEncryptionData(manifestXml));
    return decrypted.length;
}
