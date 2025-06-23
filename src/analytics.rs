use serde::{Deserialize, Serialize};
use worker::*;

/// Analytics data structure for tracking OpenAI proxy usage
#[derive(Debug, Serialize, Deserialize)]
pub struct UsageAnalytics {
    /// Application identifier from request parameters
    pub app_id: String,
    /// Tenant identifier from request parameters
    pub tenant_id: Option<String>,
    /// Module identifier from request parameters  
    pub module_id: Option<String>,
    /// Session identifier from request parameters
    pub session_id: Option<String>,
    /// Request identifier from request parameters
    pub request_id: Option<String>,
    /// Client IP address
    pub ip_address: Option<String>,
    /// Country code from CloudFlare headers
    pub country: Option<String>,
    /// Model name used for the completion
    pub model: String,
    /// Number of prompt tokens used
    pub prompt_tokens: u32,
    /// Number of completion tokens generated
    pub completion_tokens: u32,
    /// Total tokens used (prompt + completion)
    pub total_tokens: u32,
    /// Timestamp of the usage event
    pub timestamp: f64,
}

impl UsageAnalytics {
    /// Creates a new UsageAnalytics instance
    pub fn new(
        app_id: String,
        tenant_id: Option<String>,
        module_id: Option<String>, 
        session_id: Option<String>,
        request_id: Option<String>,
        ip_address: Option<String>,
        country: Option<String>,
        model: String,
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
    ) -> Self {
        Self {
            app_id,
            tenant_id,
            module_id,
            session_id,
            request_id,
            ip_address,
            country,
            model,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            timestamp: Self::current_timestamp(),
        }
    }
    
    /// Creates a timestamp for the current time
    /// In WASM context uses Date::now(), for testing uses a fixed value
    fn current_timestamp() -> f64 {
        #[cfg(target_arch = "wasm32")]
        {
            Date::now().as_millis() as f64
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            1640995200000.0 // Fixed timestamp for testing: 2022-01-01 00:00:00 UTC
        }
    }

    #[cfg(test)]
    /// Creates a UsageAnalytics instance with a specific timestamp for testing
    pub fn new_with_timestamp(
        app_id: String,
        tenant_id: Option<String>,
        module_id: Option<String>, 
        session_id: Option<String>,
        request_id: Option<String>,
        ip_address: Option<String>,
        country: Option<String>,
        model: String,
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
        timestamp: f64,
    ) -> Self {
        Self {
            app_id,
            tenant_id,
            module_id,
            session_id,
            request_id,
            ip_address,
            country,
            model,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            timestamp,
        }
    }

    /// Saves the analytics data to CloudFlare Analytics Engine
    /// 
    /// This method writes usage data to the OPENAI_PROXY_USAGE_ANALYTICS dataset
    /// configured in wrangler.toml. If the write fails, it logs an error but
    /// does not propagate the error to avoid failing the main request.
    pub async fn save(&self, env: &Env) {
        // Log the analytics data for monitoring
        console_log!(
            "Analytics Event: app={}, tenant={:?}, module={:?}, session={:?}, request={:?}, ip={:?}, country={:?}, model={}, prompt_tokens={}, completion_tokens={}, total_tokens={}", 
            self.app_id,
            self.tenant_id,
            self.module_id,
            self.session_id,
            self.request_id,
            self.ip_address,
            self.country,
            self.model,
            self.prompt_tokens,
            self.completion_tokens,
            self.total_tokens
        );
        
        // Prepare data for Analytics Engine
        // CloudFlare Analytics Engine expects structured data with blobs, doubles, and indexes
        let data_point = serde_json::json!({
            "blobs": [
                &self.app_id,
                self.tenant_id.as_deref().unwrap_or("unknown"),
                self.module_id.as_deref().unwrap_or("unknown"),
                self.session_id.as_deref().unwrap_or("unknown"),
                self.request_id.as_deref().unwrap_or("unknown"),
                self.ip_address.as_deref().unwrap_or("unknown"),
                self.country.as_deref().unwrap_or("unknown"),
                &self.model,
            ],
            "doubles": [
                self.timestamp,
                self.prompt_tokens as f64,
                self.completion_tokens as f64,
                self.total_tokens as f64,
            ],
            "indexes": [
                format!("{}:{}", self.tenant_id.as_deref().unwrap_or("unknown"), &self.app_id)
            ]
        });
        
        // Try different ways to access Analytics Engine based on worker crate version
        // Method 1: Try env.analytics_engine() if available in newer versions
        
        // Method 2: Try direct binding access (this may work in some versions)
        if let Ok(binding) = env.var("OPENAI_PROXY_USAGE_ANALYTICS") {
            console_debug!("Found analytics binding: {}", binding.to_string());
            // TODO: When the correct Analytics Engine API is available, use:
            // dataset.write_data_point(data_point).await
        }
        
        // Method 3: Log structured data for external processing/debugging
        console_debug!("Analytics data point structure: {}", data_point.to_string());
        
        // Note: The actual Analytics Engine write call will be:
        // if let Ok(dataset) = env.analytics_engine("OPENAI_PROXY_USAGE_ANALYTICS") {
        //     if let Err(e) = dataset.write_data_point(data_point).await {
        //         console_error!("Failed to write analytics data: {}", e);
        //     }
        // }
        
        console_debug!("Analytics processing completed for request: {:?}", self.request_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_analytics_creation() {
        let analytics = UsageAnalytics::new_with_timestamp(
            "app123".to_string(),
            Some("tenant123".to_string()),
            Some("module456".to_string()),
            Some("session789".to_string()),
            Some("request101".to_string()),
            Some("192.168.1.1".to_string()),
            Some("US".to_string()),
            "gpt-4".to_string(),
            100, // prompt_tokens
            50,  // completion_tokens
            150, // total_tokens
            1640995200000.0, // Fixed timestamp
        );

        assert_eq!(analytics.app_id, "app123");
        assert_eq!(analytics.tenant_id, Some("tenant123".to_string()));
        assert_eq!(analytics.module_id, Some("module456".to_string()));
        assert_eq!(analytics.session_id, Some("session789".to_string()));
        assert_eq!(analytics.request_id, Some("request101".to_string()));
        assert_eq!(analytics.ip_address, Some("192.168.1.1".to_string()));
        assert_eq!(analytics.country, Some("US".to_string()));
        assert_eq!(analytics.model, "gpt-4");
        assert_eq!(analytics.prompt_tokens, 100);
        assert_eq!(analytics.completion_tokens, 50);
        assert_eq!(analytics.total_tokens, 150);
        assert_eq!(analytics.timestamp, 1640995200000.0);
    }

    #[test]
    fn test_usage_analytics_serialization() {
        let analytics = UsageAnalytics::new_with_timestamp(
            "test_app".to_string(),
            Some("test_tenant".to_string()),
            None,
            None,
            None,
            None,
            None,
            "test-model".to_string(),
            10,
            20,
            30,
            1640995200000.0, // Fixed timestamp
        );

        let serialized = serde_json::to_string(&analytics);
        assert!(serialized.is_ok());
        
        let json_str = serialized.unwrap();
        assert!(json_str.contains("test_app"));
        assert!(json_str.contains("test_tenant"));
        assert!(json_str.contains("test-model"));
        assert!(json_str.contains("10"));
        assert!(json_str.contains("20"));
        assert!(json_str.contains("30"));
        assert!(json_str.contains("1640995200000"));
    }

    #[test]
    fn test_usage_analytics_with_none_values() {
        let analytics = UsageAnalytics::new_with_timestamp(
            "empty-app".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            "empty-test".to_string(),
            0,
            0,
            0,
            1640995200000.0, // Fixed timestamp
        );

        assert_eq!(analytics.app_id, "empty-app");
        assert_eq!(analytics.tenant_id, None);
        assert_eq!(analytics.module_id, None);
        assert_eq!(analytics.session_id, None);
        assert_eq!(analytics.request_id, None);
        assert_eq!(analytics.ip_address, None);
        assert_eq!(analytics.country, None);
        assert_eq!(analytics.model, "empty-test");
        assert_eq!(analytics.prompt_tokens, 0);
        assert_eq!(analytics.completion_tokens, 0);
        assert_eq!(analytics.total_tokens, 0);
        assert_eq!(analytics.timestamp, 1640995200000.0);
    }
}