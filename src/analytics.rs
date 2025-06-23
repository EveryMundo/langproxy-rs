use serde::{Deserialize, Serialize};
use worker::*;

/// Analytics data structure for tracking OpenAI proxy usage
#[derive(Debug, Serialize, Deserialize)]
pub struct UsageAnalytics {
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
            timestamp: Date::now().as_millis() as f64,
        }
    }

    /// Saves the analytics data to CloudFlare Analytics Engine
    /// 
    /// This method writes usage data to the OPENAI_PROXY_USAGE_ANALYTICS dataset
    /// configured in wrangler.toml. If the write fails, it logs an error but
    /// does not propagate the error to avoid failing the main request.
    pub async fn save(&self, env: &Env) {
        // For now, just log the analytics data until we figure out the correct API
        console_log!(
            "Analytics data: tenant={:?}, module={:?}, session={:?}, request={:?}, ip={:?}, country={:?}, model={}, tokens={}/{}/{}", 
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
        
        // TODO: Implement actual Analytics Engine write when we determine the correct API
        // The binding OPENAI_PROXY_USAGE_ANALYTICS is configured in wrangler.toml
        // but the worker crate might not have the analytics_engine method yet
        
        console_debug!("Analytics data logged for request: {:?}", self.request_id);
    }
}