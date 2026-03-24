use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage};
use serde_json::Value;

use crate::step_registry::{StepHandler, StepRegistry};

// ---------------------------------------------------------------------------
// DownloadStep (stub)
// ---------------------------------------------------------------------------

/// DownloadStep extracts URLs from data and prepares download metadata.
///
/// This is currently a stub implementation that identifies downloadable URLs
/// from the incoming data and returns them with download path annotations.
pub struct DownloadStep;

#[async_trait]
impl StepHandler for DownloadStep {
    fn name(&self) -> &'static str {
        "download"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        _args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let download_type = match params {
            Value::Object(obj) => obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("media"),
            Value::String(s) => s.as_str(),
            _ => "media",
        };

        // Extract URL from params or data
        let url = match params {
            Value::Object(obj) => obj.get("url").and_then(|v| v.as_str()).map(String::from),
            _ => None,
        };

        // If no URL in params, try to extract from data
        let url = url.or_else(|| {
            match data {
                Value::String(s) => Some(s.clone()),
                Value::Object(obj) => obj
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                _ => None,
            }
        });

        // Build result with download metadata
        let mut result = match data {
            Value::Object(obj) => obj.clone(),
            _ => serde_json::Map::new(),
        };

        result.insert(
            "download_type".to_string(),
            Value::String(download_type.to_string()),
        );

        if let Some(u) = url {
            result.insert("download_url".to_string(), Value::String(u.clone()));
            // Derive a filename from the URL
            let filename = u
                .rsplit('/')
                .next()
                .unwrap_or("download")
                .split('?')
                .next()
                .unwrap_or("download");
            result.insert(
                "download_path".to_string(),
                Value::String(filename.to_string()),
            );
        }

        result.insert(
            "download_status".to_string(),
            Value::String("pending".to_string()),
        );

        Ok(Value::Object(result))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_download_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(DownloadStep));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_args() -> HashMap<String, Value> {
        HashMap::new()
    }

    #[tokio::test]
    async fn test_download_step_registers() {
        let mut registry = StepRegistry::new();
        register_download_steps(&mut registry);
        assert!(registry.get("download").is_some());
    }

    #[test]
    fn test_download_is_browser_step() {
        assert!(DownloadStep.is_browser_step());
    }

    #[tokio::test]
    async fn test_download_with_url_in_params() {
        let step = DownloadStep;
        let params = json!({"type": "media", "url": "https://example.com/video.mp4"});
        let result = step
            .execute(None, &params, &json!(null), &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_url"], "https://example.com/video.mp4");
        assert_eq!(result["download_path"], "video.mp4");
        assert_eq!(result["download_type"], "media");
        assert_eq!(result["download_status"], "pending");
    }

    #[tokio::test]
    async fn test_download_with_url_in_data() {
        let step = DownloadStep;
        let params = json!({"type": "article"});
        let data = json!({"url": "https://example.com/article.pdf", "title": "Test"});
        let result = step
            .execute(None, &params, &data, &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_url"], "https://example.com/article.pdf");
        assert_eq!(result["download_path"], "article.pdf");
        assert_eq!(result["download_type"], "article");
        assert_eq!(result["title"], "Test");
    }

    #[tokio::test]
    async fn test_download_no_url() {
        let step = DownloadStep;
        let result = step
            .execute(None, &json!(null), &json!(null), &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_status"], "pending");
        assert!(result.get("download_url").is_none());
    }
}
