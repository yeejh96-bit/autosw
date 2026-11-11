//! Derived from kordoc svg-render.mjs, MIT, Copyright 2026 chrisryugj.
//! Picture symbols, clipping and rotation. See data/kordoc-LICENSE.
#[cfg(test)]
use super::layout_text::LayoutText;
use super::{
    numbering::{nullish, truth},
    para_model::{child, find_first, num},
    shape_geometry::{GeometryBox, object_geometry},
    style_dom::ParsedXml,
    svg_para::RenderContext,
    value::{Value, native_json as json, number, string},
};

fn pt(v: &Value) -> String {
    let x = number(v);
    let rounded = if !x.is_finite() || x == 0.0 || x.abs() >= 4503599627370496.0 {
        x
    } else if (-0.5..0.0).contains(&x) {
        -0.0
    } else {
        let f = x.floor();
        if x - f < 0.5 { f } else { f + 1.0 }
    };
    string(&(rounded / 100.0).into())
}

pub fn image_symbol(loaded: &mut Value, defs: &mut Vec<Value>) -> Result<Value, String> {
    if !truth(&loaded["symId"]) {
        if !loaded.is_object() {
            return Err("이미지 정보 객체가 필요합니다.".into());
        }
        let id = format!("bin{}", defs.len());
        loaded["symId"] = id.clone().into();
        defs.push(format!("<symbol id=\"{id}\" viewBox=\"0 0 100 100\" preserveAspectRatio=\"none\"><image width=\"100\" height=\"100\" preserveAspectRatio=\"none\" href=\"{}\"/></symbol>",string(&loaded["dataUri"])).into());
    }
    Ok(loaded["symId"].clone())
}

pub fn draw_pic(
    doc: &ParsedXml,
    pic: usize,
    mut x: Value,
    mut y: Value,
    ctx: &mut RenderContext,
) -> Result<(), String> {
    let size = child(doc, pic, "sz");
    let mut w: Value = num(doc, size, "width", 5669.0).into();
    let mut h: Value = num(doc, size, "height", 5669.0).into();
    let cur = child(doc, pic, "curSz");
    let cw = num(doc, cur, "width", 0.0);
    let ch = num(doc, cur, "height", 0.0);
    let geom = object_geometry(
        doc,
        pic,
        &GeometryBox {
            x: x.clone(),
            y: y.clone(),
            w: if cw == 0.0 || cw.is_nan() {
                w.clone()
            } else {
                cw.into()
            },
            h: if ch == 0.0 || ch.is_nan() {
                h.clone()
            } else {
                ch.into()
            },
            box_w: Some(w.clone()),
            box_h: Some(h.clone()),
        },
    );
    let rotated = truth(&geom["rotation"]);
    if rotated {
        x = geom["x"].clone();
        y = geom["y"].clone();
        w = geom["w"].clone();
        h = geom["h"].clone();
        ctx.region_rotations.push(geom["rotation"].clone());
    }
    draw_pic_frame(doc, pic, x, y, w, h, &geom["transform"], ctx)?;
    // Match the original: a failure above leaves its rotation in place.
    if rotated {
        ctx.region_rotations.pop();
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn draw_pic_frame(
    doc: &ParsedXml,
    pic: usize,
    x: Value,
    y: Value,
    w: Value,
    h: Value,
    transform: &Value,
    ctx: &mut RenderContext,
) -> Result<(), String> {
    let reference = find_first(doc, pic, "img", 0)
        .map(|img| {
            doc.nodes[img]
                .attr("binaryItemIDRef")
                .map(Value::from)
                .unwrap_or(Value::Null)
        })
        .unwrap_or(Value::Undefined);
    let key = if nullish(&reference) {
        None
    } else {
        Some(string(&reference))
    };
    let loaded = key
        .as_ref()
        .and_then(|key| ctx.images.get(key))
        .is_some_and(truth);
    let bbox = ctx.bbox_value(&x, &y, &w, &h)?;
    let id = ctx.regions.add("image",bbox,&json!({"sourceId":if nullish(&reference){Value::Undefined}else{reference.clone()},"parentId":ctx.parent_stack.last().map(|s|Value::from(s.as_str())).unwrap_or(Value::Undefined)}));
    ctx.emit(
        format!(
            "<g data-kordoc-id=\"{id}\" data-kordoc-type=\"image\" data-kordoc-page=\"{}\">",
            string(&ctx.page_no().into())
        )
        .into(),
    )?;
    let close = if truth(transform) { "</g></g>" } else { "</g>" };
    if truth(transform) {
        ctx.emit(format!("<g transform=\"{}\">", string(transform)).into())?;
    }
    if !loaded {
        ctx.emit(format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#eee\" stroke=\"#c00\" stroke-width=\"0.5\"/>",pt(&x),pt(&y),pt(&w),pt(&h)).into())?;
        ctx.warn_once(
            &format!("img:{}", string(&reference)),
            &format!(
                "이미지 바이너리 누락: {}",
                if nullish(&reference) {
                    "(ref 없음)".into()
                } else {
                    string(&reference)
                }
            ),
        );
        ctx.emit(close.into())?;
        return Ok(());
    }
    ctx.stats["images"] = (number(&ctx.stats["images"]) + 1.0).into();
    let clip = child(doc, pic, "imgClip");
    let dim = child(doc, pic, "imgDim");
    let org = child(doc, pic, "orgSz");
    let dw = num(doc, dim, "dimwidth", 0.0);
    let dh = num(doc, dim, "dimheight", 0.0);
    let rw = if dw > 0.0 {
        dw
    } else {
        num(doc, org, "width", 0.0)
    };
    let rh = if dh > 0.0 {
        dh
    } else {
        num(doc, org, "height", 0.0)
    };
    let cl = num(doc, clip, "left", 0.0);
    let ct = num(doc, clip, "top", 0.0);
    let cr = num(doc, clip, "right", rw);
    let cb = num(doc, clip, "bottom", rh);
    let cropped = rw > 0.0
        && rh > 0.0
        && clip.is_some()
        && (cl > 0.0 || ct > 0.0 || cr < rw || cb < rh)
        && cr > cl
        && cb > ct;
    let key = key.ok_or("이미지 참조가 없습니다.")?;
    let symbol = image_symbol(
        ctx.images.get_mut(&key).ok_or("이미지 정보가 없습니다.")?,
        &mut ctx.defs,
    )?;
    let symbol = string(&symbol);
    let svg = if cropped {
        format!(
            "<svg x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\" preserveAspectRatio=\"none\"><use href=\"#{symbol}\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"/></svg>",
            pt(&x),
            pt(&y),
            pt(&w),
            pt(&h),
            pt(&cl.into()),
            pt(&ct.into()),
            pt(&(cr - cl).into()),
            pt(&(cb - ct).into()),
            pt(&rw.into()),
            pt(&rh.into())
        )
    } else {
        format!(
            "<use href=\"#{symbol}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
            pt(&x),
            pt(&y),
            pt(&w),
            pt(&h)
        )
    };
    ctx.emit(svg.into())?;
    ctx.emit(close.into())?;
    Ok(())
}

pub fn sniff_mime(name: &str, bytes: &[u8]) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".png") || bytes.len() > 4 && bytes.starts_with(&[0x89, 0x50]) {
        "image/png"
    } else if lower.ends_with(".bmp") || bytes.len() > 2 && bytes.starts_with(&[0x42, 0x4d]) {
        "image/bmp"
    } else if lower.ends_with(".gif") || bytes.len() > 3 && bytes.starts_with(&[0x47, 0x49, 0x46]) {
        "image/gif"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
fn selected(doc: &ParsedXml, path: &Value) -> Result<usize, String> {
    let mut at = doc.root;
    for index in path.as_array().into_iter().flatten() {
        at = *doc.nodes[at]
            .children
            .get(index.as_u64().ok_or("노드 경로 오류")? as usize)
            .ok_or("노드 경로 범위 오류")?;
    }
    Ok(at)
}
#[cfg(test)]
pub(super) fn run(op: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    let i = Value::from_json(input);
    if op == "sniffMime" {
        let bytes = i["bytes"]
            .as_array()
            .ok_or("bytes 배열 필요")?
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|v| u8::try_from(v).ok())
                    .ok_or("byte 범위 오류")
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(sniff_mime(i["name"].as_str().ok_or("name 문자열 필요")?, &bytes).into());
    }
    let doc = super::style_dom::parse_style_xml(i["xml"].as_str().unwrap_or("<pic/>"))?;
    let init = &i["context"];
    let count = init["pageCount"].as_u64().unwrap_or(3) as usize;
    if count > 100_000 {
        return Err("출력 쪽 수 한도 초과".into());
    }
    let mut ctx = RenderContext::new(json!({}), count);
    for key in ["page", "pageBase", "pageH", "stats"] {
        if let Some(v) = init.get(key) {
            match key {
                "page" => ctx.page = v.clone(),
                "pageBase" => ctx.page_base = v.clone(),
                "pageH" => ctx.page_h = number(v),
                _ => ctx.stats = v.clone(),
            }
        }
    }
    if let Some(v) = init["defs"].as_array() {
        ctx.defs = v.clone();
    }
    if let Some(v) = init["rotations"].as_array() {
        ctx.region_rotations = v.clone();
    }
    if let Some(v) = init["parents"].as_array() {
        ctx.parent_stack = v.iter().map(string).collect();
    }
    for pair in i["images"].as_array().into_iter().flatten() {
        ctx.images.insert(string(&pair[0]), pair[1].clone());
    }
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
    if let Some(dx) = init.get("bodyDx") {
        let dx = dx.clone();
        let calls = calls.clone();
        let fail = init["throwBodyAt"].as_u64();
        ctx.body_dx = Some(Box::new(move |page| {
            let at = calls.borrow().len();
            calls.borrow_mut().push(page.clone());
            if fail == Some(at as u64) {
                return Err("synthetic bodyDx failure".into());
            }
            Ok(dx.clone())
        }));
    }
    let draws = if op == "sequence" {
        i["draws"].as_array().ok_or("draws 배열 필요")?.clone()
    } else {
        vec![i.clone()]
    };
    let mut errors = Vec::new();
    let mut results = Vec::new();
    for draw in &draws {
        let action = if op == "sequence" {
            draw["operation"].as_str().unwrap_or("drawPic")
        } else {
            op
        };
        let result = (|| {
            if action == "imageSymbol" {
                let key = string(&draw["key"]);
                return image_symbol(
                    ctx.images.get_mut(&key).ok_or("이미지 정보 누락")?,
                    &mut ctx.defs,
                );
            }
            let node = selected(&doc, &draw["path"])?;
            match action {
                "drawPic" => draw_pic(&doc, node, draw["x"].clone(), draw["y"].clone(), &mut ctx)?,
                "drawPicFrame" => draw_pic_frame(
                    &doc,
                    node,
                    draw["x"].clone(),
                    draw["y"].clone(),
                    draw["w"].clone(),
                    draw["h"].clone(),
                    &draw["transform"],
                    &mut ctx,
                )?,
                _ => return Err("알 수 없는 이미지 연산".into()),
            }
            Ok(Value::Undefined)
        })();
        errors.push(result.is_err());
        results.push(result.unwrap_or(Value::Undefined));
    }
    let pages: Vec<Vec<Value>> = ctx
        .pages
        .iter()
        .map(|p| p.iter().map(|s| json!(s.0)).collect())
        .collect();
    let validation: Vec<bool> = ctx
        .pages
        .iter()
        .map(|page| {
            let mut svg = LayoutText::from("<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>");
            for def in &ctx.defs {
                svg.append(&LayoutText::from(string(def)).as_text_ref());
            }
            svg.append(&LayoutText::from("</defs>").as_text_ref());
            for (index, piece) in page.iter().enumerate() {
                if index > 0 {
                    svg.append(&LayoutText::from("\n").as_text_ref());
                }
                svg.append(&piece.as_text_ref());
            }
            svg.append(&LayoutText::from("</svg>").as_text_ref());
            svg.to_string_strict()
                .and_then(|s| super::svg_safety::validate_rendered_svg(&s))
                .is_ok()
        })
        .collect();
    Ok(json!({"pages":pages,"defs":ctx.defs,"images":ctx.images.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"regions":ctx.regions.regions,"stats":ctx.stats,"warnings":ctx.warnings,"warned":ctx.warned_order,"rotations":ctx.region_rotations,"bodyDxCalls":calls.borrow().clone(),"errors":errors,"results":results,"validation":validation}).to_json())
}
