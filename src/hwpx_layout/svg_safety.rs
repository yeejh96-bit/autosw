//! Preserve the layout preview's existing XML/element/attribute allow rules.
use crate::{input::is_space, office_xml::parse_office_xml};
#[cfg(test)]
use serde_json::Value;

fn image_reference(value: &str) -> bool {
    ["png", "jpeg", "gif", "webp"].iter().any(|kind| {
        value
            .strip_prefix(&format!("data:image/{kind};base64,"))
            .is_some_and(|body| {
                !body.is_empty()
                    && body.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=') || is_space(c)
                    })
            })
    })
}
fn local_url(value: &str) -> bool {
    value
        .strip_prefix("url(#")
        .and_then(|tail| tail.strip_suffix(')'))
        .is_some_and(|id| {
            !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        })
}

pub fn validate_rendered_svg(svg: &str) -> Result<(), String> {
    let parsed = parse_office_xml(svg)?;
    for node in parsed.nodes {
        if [
            "script",
            "foreignObject",
            "iframe",
            "style",
            "animate",
            "set",
        ]
        .contains(&node.local.as_str())
        {
            return Err("미리보기의 실행 요소 차단".into());
        }
        for (key, value) in node.attrs {
            let value = value.as_str().ok_or("잘못된 SVG 속성 값")?;
            if key.to_ascii_lowercase().starts_with("on") {
                return Err("미리보기 이벤트 차단".into());
            }
            if (key == "href" || key.ends_with(":href"))
                && !value.starts_with('#')
                && !image_reference(value)
            {
                return Err("미리보기 외부 참조 차단".into());
            }
            if value.to_ascii_lowercase().contains("url(") && !local_url(value) {
                return Err("미리보기 외부 스타일 참조 차단".into());
            }
        }
    }
    Ok(())
}
#[cfg(test)]
pub(super) fn run(operation: &str, input: &Value) -> Result<Value, String> {
    if operation != "validateSvg" {
        return Err("알 수 없는 SVG 검사 연산입니다.".into());
    }
    validate_rendered_svg(input["svg"].as_str().ok_or("svg 문자열이 필요합니다.")?)?;
    Ok(Value::Bool(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_boundaries_keep_legacy_case_and_whitespace_rules() {
        assert!(validate_rendered_svg("<svg><image href='data:image/png;base64,AA==&#160;'/><path fill='url(#x_-2)'/></svg>").is_ok());
        for svg in [
            "<svg><script/></svg>",
            "<svg onload='x'/>",
            "<svg href='https://example.invalid'/>",
            "<svg fill='URL(#x)'/>",
            "<svg fill='url(#한글)'/>",
        ] {
            assert!(validate_rendered_svg(svg).is_err());
        }
        // These case-sensitive legacy allow rules are intentional compatibility.
        assert!(validate_rendered_svg("<svg><SCRIPT/><image HREF='unused'/></svg>").is_ok());
    }
}
