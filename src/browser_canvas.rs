//! Browser canvas font measurement; process/CDP ownership is shared with PDF.
use crate::browser_session::{BrowserSession, LaunchPolicy};
use serde_json::{Value, json};
pub struct Canvas {
    browser: BrowserSession,
    context: String,
}
impl Canvas {
    pub fn launch() -> Result<Self, String> {
        let (browser, context) =
            BrowserSession::launch_with(LaunchPolicy::Canvas, Self::configure)?;
        Ok(Self { browser, context })
    }
    pub(crate) fn configure(browser: &mut BrowserSession) -> Result<String, String> {
        browser.call("Network.enable", json!({}), true)?;
        browser.call("Network.setBlockedURLs", json!({"urls":["*"]}), true)?;
        let context=browser.call("Runtime.evaluate",json!({"expression":"document.createElement('canvas').getContext('2d')","objectGroup":"my-system-canvas"}),true)?;
        context["result"]["objectId"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "글자 측정 화면 없음".to_owned())
    }
    fn measure_css(&mut self, font: &str, text: &str) -> Result<f64, String> {
        let result=self.browser.call("Runtime.callFunctionOn",json!({"objectId":self.context,"functionDeclaration":"function(font,text){this.font=font;return this.measureText(text).width;}","arguments":[{"value":font},{"value":text}],"returnByValue":true}),true)?;
        result["result"]["value"]
            .as_f64()
            .filter(|width| width.is_finite() && *width >= 0.0)
            .ok_or_else(|| "글자 폭 결과 오류".into())
    }
    pub fn require_font(&mut self, font: &str) -> Result<(), String> {
        let sample = "가나다라마바사 ABCDEFG ilMW 0123456789";
        let quoted = serde_json::to_string(font).map_err(|e| e.to_string())?;
        let mut found = false;
        for fallback in ["monospace", "serif"] {
            let baseline = self.measure_css(&format!("20px {fallback}"), sample)?;
            let actual = self.measure_css(&format!("20px {quoted},{fallback}"), sample)?;
            if (actual - baseline).abs() > 0.01 {
                found = true;
            }
        }
        if found {
            Ok(())
        } else {
            Err(format!("설치 글꼴을 확인할 수 없습니다: {font}"))
        }
    }
    pub fn measure(&mut self, text: &str, style: &Value) -> Result<f64, String> {
        let font = style["font"].as_str().ok_or("글꼴 없음")?;
        let size = style["size"].as_f64().ok_or("글자 크기 없음")?;
        let css = format!(
            "{}{}px {}",
            if style["bold"].as_bool() == Some(true) {
                "bold "
            } else {
                ""
            },
            crate::input::js_string(&json!(size * 4.0 / 3.0)),
            serde_json::to_string(font).map_err(|e| e.to_string())?
        );
        Ok(self.measure_css(&css, text)? * 3.0 / 4.0)
    }
    pub fn close(self) -> Result<(), String> {
        self.browser.close()
    }
}
