// Derived from kordoc (MIT), Copyright 2026 chrisryugj. See ../LICENSE or package-root LICENSE and PROVENANCE.json.
export function regionId(type, seq) {
    return `${type}-${String(seq).padStart(6, "0")}`;
}
export function unionBBox(a, b) {
    const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
    return {
        page: a.page,
        x,
        y,
        width: Math.max(a.x + a.width, b.x + b.width) - x,
        height: Math.max(a.y + a.height, b.y + b.height) - y
    };
}
export class RegionCollector {
    regions = [];
    counters = new Map();
    add(type, bbox, extra = {}) {
        const seq = (this.counters.get(type) ?? 0) + 1;
        this.counters.set(type, seq);
        const id = regionId(type, seq);
        const region = {
            id,
            type,
            page: bbox.page,
            regions: [
                bbox
            ]
        };
        if (extra.sourceId) region.sourceId = extra.sourceId;
        if (extra.parentId) region.parentId = extra.parentId;
        this.regions.push(region);
        return id;
    }
    addFragment(id, bbox) {
        const r = this.regions.find((x)=>x.id === id);
        if (!r) return;
        const same = r.regions.find((f)=>f.page === bbox.page);
        if (same) Object.assign(same, unionBBox(same, bbox));
        else r.regions.push(bbox);
    }
}
