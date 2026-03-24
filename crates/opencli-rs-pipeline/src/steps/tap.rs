use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage};
use serde_json::Value;

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template_str, TemplateContext};

// ---------------------------------------------------------------------------
// TapStep
// ---------------------------------------------------------------------------

/// TapStep bridges store actions (Pinia/Vuex) with network interception.
///
/// It installs a network interceptor, evaluates JS to invoke a store action,
/// then collects the intercepted response and optionally selects a nested path.
pub struct TapStep;

/// Walk a dotted path like `"data.items"` into a `Value`.
fn select_path(value: &Value, path: &str) -> Value {
    let mut current = value.clone();
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        current = current.get(segment).cloned().unwrap_or(Value::Null);
    }
    current
}

#[async_trait]
impl StepHandler for TapStep {
    fn name(&self) -> &'static str {
        "tap"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let pg = page
            .clone()
            .ok_or_else(|| CliError::pipeline("tap: requires an active page"))?;

        let obj = params
            .as_object()
            .ok_or_else(|| CliError::pipeline("tap: params must be an object"))?;

        let ctx = TemplateContext {
            args: args.clone(),
            data: data.clone(),
            item: Value::Null,
            index: 0,
        };

        // Extract action name (required)
        let action_raw = obj
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CliError::pipeline("tap: missing 'action' field"))?;
        let action_rendered = render_template_str(action_raw, &ctx)?;
        let action = action_rendered
            .as_str()
            .ok_or_else(|| CliError::pipeline("tap: action must resolve to a string"))?
            .to_string();

        // Extract args for the store action (optional)
        let action_args = obj.get("args").cloned().unwrap_or(Value::Array(vec![]));
        let action_args_json = serde_json::to_string(&action_args)
            .map_err(|e| CliError::pipeline(format!("tap: failed to serialize args: {e}")))?;

        // Extract URL pattern for interception (optional)
        let url_pattern = obj
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("*");

        // Extract select path (optional)
        let select_path_str = obj.get("select").and_then(|v| v.as_str());

        // Extract wait timeout
        let wait_ms = obj.get("wait").and_then(|v| v.as_u64()).unwrap_or(5000);

        // Step 1: Install network interceptor
        pg.intercept_requests(url_pattern).await?;

        // Step 2: Evaluate JS to invoke the store action
        // This JS tries common store patterns: Pinia (__pinia), Vuex ($store), or window dispatch
        let js = format!(
            r#"(async () => {{
                const args = {action_args_json};
                // Try to find and invoke the store action
                const parts = "{action}".split(".");
                let target = window;
                for (const part of parts) {{
                    target = target && target[part];
                }}
                if (typeof target === 'function') {{
                    return await target(...args);
                }}
                // Fallback: try evaluating as a direct expression
                return await eval("{action}(" + args.map(a => JSON.stringify(a)).join(",") + ")");
            }})()"#,
        );
        let eval_result = pg.evaluate(&js).await?;

        // Step 3: Wait and collect intercepted responses
        pg.wait_for_timeout(wait_ms).await?;
        let requests = pg.get_intercepted_requests().await?;

        // Determine result: prefer intercepted response body, fallback to eval result
        let result = if !requests.is_empty() {
            // Try to parse the first intercepted request's body as JSON
            if let Some(body) = &requests[0].body {
                serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.clone()))
            } else {
                eval_result
            }
        } else {
            eval_result
        };

        // Step 4: Optionally select a nested path
        if let Some(path) = select_path_str {
            Ok(select_path(&result, path))
        } else {
            Ok(result)
        }
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_tap_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(TapStep));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_tap_step_registers() {
        let mut registry = StepRegistry::new();
        register_tap_steps(&mut registry);
        assert!(registry.get("tap").is_some());
    }

    #[test]
    fn test_tap_is_browser_step() {
        assert!(TapStep.is_browser_step());
    }

    #[test]
    fn test_select_path() {
        let val = json!({"data": {"items": [1, 2, 3]}});
        assert_eq!(select_path(&val, "data.items"), json!([1, 2, 3]));
        assert_eq!(select_path(&val, "data"), json!({"items": [1, 2, 3]}));
        assert_eq!(select_path(&val, "missing"), Value::Null);
    }

    #[tokio::test]
    async fn test_tap_requires_page() {
        let step = TapStep;
        let params = json!({"action": "store.fetchData"});
        let result = step
            .execute(None, &params, &json!(null), &HashMap::new())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tap_requires_object_params() {
        let step = TapStep;
        let result = step
            .execute(None, &json!("invalid"), &json!(null), &HashMap::new())
            .await;
        assert!(result.is_err());
    }
}
