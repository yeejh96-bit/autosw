// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
import { toInt32 } from "./layout.mjs";
export function cachedParagraphBottom(lsa, bodyFlow) {
    let bottom = Number.NEGATIVE_INFINITY, previousV = Number.NEGATIVE_INFINITY;
    for(let i = 0; i < lsa.childNodes.length; i++){
        const seg = lsa.childNodes[i];
        if (seg.nodeType !== 1 || seg.localName !== "lineseg") continue;
        const num = (name, fallback = 0)=>toInt32(seg.getAttribute(name) ?? undefined, fallback);
        const v = num("vertpos");
        if (bodyFlow && v < previousV) bottom = Number.NEGATIVE_INFINITY;
        bottom = Math.max(bottom, v + Math.max(num("vertsize", 1000), num("textheight", 1000)) + num("spacing"));
        previousV = v;
    }
    return bottom;
}
