//! Derived from kordoc (MIT), Copyright 2026 chrisryugj.
//! Original render/scene.mjs. See data/kordoc-LICENSE and data/kordoc-PROVENANCE.json.
use super::{
    layout::{maximum, minimum},
    numbering::{add as js_add, truth},
    value::{Map, Value, number, string},
};
#[cfg(test)]
use super::{numbering::nullish, value::native_json as json};

pub fn region_id(kind: &str, sequence: &str) -> String {
    let padding = 6_usize.saturating_sub(sequence.encode_utf16().count());
    format!("{kind}-{}{sequence}", "0".repeat(padding))
}
/// Keeps the first box's page, matching the original insertion order and JS numeric coercion.
pub fn union_bbox(a: &Value, b: &Value) -> Value {
    let x = minimum(number(&a["x"]), number(&b["x"]));
    let y = minimum(number(&a["y"]), number(&b["y"]));
    let mut out = Map::new();
    if let Some(page) = a.get("page") {
        out.insert("page".into(), page.clone());
    }
    out.insert("x".into(), x.into());
    out.insert("y".into(), y.into());
    out.insert(
        "width".into(),
        Value::from(
            maximum(
                number(&js_add(&a["x"], &a["width"])),
                number(&js_add(&b["x"], &b["width"])),
            ) - x,
        ),
    );
    out.insert(
        "height".into(),
        Value::from(
            maximum(
                number(&js_add(&a["y"], &a["height"])),
                number(&js_add(&b["y"], &b["height"])),
            ) - y,
        ),
    );
    Value::Object(out)
}
#[derive(Clone, Default)]
pub struct RegionCollector {
    pub regions: Vec<Value>,
    counters: Vec<(Option<Value>, usize)>,
}
fn strict_equal(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None | Some(Value::Undefined), None | Some(Value::Undefined)) => true,
        (Some(Value::Number(a)), Some(Value::Number(b))) => a == b,
        (Some(Value::Object(_) | Value::Array(_)), _)
        | (_, Some(Value::Object(_) | Value::Array(_))) => false,
        _ => a == b,
    }
}
fn same_counter_key(a: Option<&Value>, b: Option<&Value>) -> bool {
    strict_equal(a, b)
        || matches!((a,b),(Some(Value::Number(a)),Some(Value::Number(b))) if a.is_nan() && b.is_nan())
}
impl RegionCollector {
    pub fn add(&mut self, kind: &str, bbox: Value, extra: &Value) -> String {
        self.add_value(Some(&Value::String(kind.into())), bbox, extra)
    }
    pub fn add_value(&mut self, kind: Option<&Value>, bbox: Value, extra: &Value) -> String {
        let counter = self
            .counters
            .iter()
            .position(|(stored, _)| same_counter_key(stored.as_ref(), kind));
        let sequence = if let Some(index) = counter {
            self.counters[index].1 += 1;
            self.counters[index].1
        } else {
            self.counters.push((kind.cloned(), 1));
            1
        };
        let id = region_id(
            &string(kind.unwrap_or(&Value::Undefined)),
            &sequence.to_string(),
        );
        let mut region = Map::new();
        region.insert("id".into(), Value::String(id.clone()));
        if let Some(kind) = kind {
            region.insert("type".into(), kind.clone());
        }
        if let Some(page) = bbox.get("page") {
            region.insert("page".into(), page.clone());
        }
        region.insert("regions".into(), Value::Array(vec![bbox]));
        for name in ["sourceId", "parentId"] {
            if truth(&extra[name]) {
                region.insert(name.into(), extra[name].clone());
            }
        }
        self.regions.push(Value::Object(region));
        id
    }
    pub fn add_fragment(&mut self, id: &str, bbox: Value) {
        let Some(region) = self
            .regions
            .iter_mut()
            .find(|region| region["id"].as_str() == Some(id))
        else {
            return;
        };
        let fragments = region["regions"]
            .as_array_mut()
            .expect("collector regions array");
        if let Some(same) = fragments
            .iter_mut()
            .find(|fragment| strict_equal(fragment.get("page"), bbox.get("page")))
        {
            let union = union_bbox(same, &bbox);
            if let Some(same) = same.as_object_mut() {
                for (key, value) in union.as_object().expect("union box") {
                    same.insert(key.clone(), value.clone());
                }
            }
        } else {
            fragments.push(bbox);
        }
    }
}
/// collector actions: {op:"add",type,bbox,extra?} or {op:"addFragment",id,bbox}.
/// Returns {ids,regions}; ids contains only results of add, in call order.
#[cfg(test)]
pub(super) fn run(operation: &str, input: &serde_json::Value) -> Result<serde_json::Value, String> {
    run_native(operation, &Value::from_json(input)).map(|value| value.to_json())
}
#[cfg(test)]
fn run_native(operation: &str, input: &Value) -> Result<Value, String> {
    match operation {
        "regionId" => Ok(Value::String(region_id(
            &string(&input["type"]),
            &string(&input["seq"]),
        ))),
        "unionBBox" => {
            if nullish(&input["a"]) || nullish(&input["b"]) {
                return Err("Cannot read properties of null or undefined".into());
            }
            Ok(union_bbox(&input["a"], &input["b"]))
        }
        "collector" => {
            let mut collector = RegionCollector::default();
            let mut ids = Vec::new();
            for action in input["actions"]
                .as_array()
                .ok_or("actions must be an array")?
            {
                match action["op"].as_str() {
                    Some("add") => {
                        if nullish(&action["bbox"]) {
                            return Err("Cannot read properties of null or undefined".into());
                        }
                        if action.get("extra").is_some_and(Value::is_null) {
                            return Err("Cannot read properties of null".into());
                        }
                        ids.push(collector.add_value(
                            action.get("type"),
                            action["bbox"].clone(),
                            &action["extra"],
                        ));
                    }
                    Some("addFragment") => {
                        if let Some(id) = action["id"].as_str() {
                            if nullish(&action["bbox"])
                                && collector.regions.iter().any(|region| region["id"] == id)
                            {
                                return Err("Cannot read properties of null or undefined".into());
                            }
                            collector.add_fragment(id, action["bbox"].clone());
                        }
                    }
                    _ => return Err("unknown collector action".into()),
                }
            }
            Ok(json!({"ids":ids,"regions":collector.regions}))
        }
        _ => Err(format!("알 수 없는 scene 연산: {operation}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn padding_counts_utf16() {
        assert_eq!(region_id("p", "😀a"), "p-000😀a");
    }
    #[test]
    fn native_boxes_keep_nonfinite_values_across_repeated_unions() {
        let a = json!({"page":1,"x":0,"y":0,"height":1});
        let b = json!({"page":1,"x":2,"y":3,"width":4,"height":5});
        let first = union_bbox(&a, &b);
        assert!(number(&first["width"]).is_nan());
        let second = union_bbox(&first, &b);
        assert!(number(&second["width"]).is_nan());
        let zero = union_bbox(&json!({"x":-0.0,"y":0.0}), &json!({"x":0.0,"y":-0.0}));
        assert!(number(&zero["x"]).is_sign_negative());
        assert!(number(&zero["y"]).is_sign_negative());
        let mut collector = RegionCollector::default();
        let nan = Value::from(f64::NAN);
        let bbox = json!({"page":f64::NAN,"x":0,"y":0,"width":1,"height":1});
        let id = collector.add_value(Some(&nan), bbox.clone(), &Value::Undefined);
        assert_eq!(
            collector.add_value(Some(&nan), bbox.clone(), &Value::Undefined),
            "NaN-000002"
        );
        collector.add_fragment(&id, bbox);
        assert_eq!(collector.regions[0]["regions"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn fragments_keep_metadata_and_order() {
        let mut collector = RegionCollector::default();
        let id = collector.add(
            "p",
            json!({"page":2,"x":3,"y":4,"width":5,"height":6,"tag":"keep"}),
            &json!({"sourceId":"s"}),
        );
        collector.add_fragment(&id, json!({"page":2,"x":1,"y":2,"width":1,"height":1}));
        collector.add_fragment(&id, json!({"page":1,"x":0,"y":0,"width":2,"height":2}));
        assert_eq!(collector.regions[0]["page"], 2);
        assert_eq!(collector.regions[0]["regions"][0]["tag"], "keep");
        assert_eq!(collector.regions[0]["regions"][0]["width"], 7.0);
        assert_eq!(collector.regions[0]["regions"][1]["page"], 1);
    }
}
