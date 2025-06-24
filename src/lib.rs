// Copyright (c) 2025 PROS Inc.
// All rights reserved.

use serde::{Deserialize, Serialize};
use serde_json::json;
// use hashbrown::HashMap;
use futures_util::StreamExt;
use heapless::String as HString;

use worker::*;

mod analytics;
use analytics::UsageAnalytics;

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    // Create an instance of the Router, which can use parameters (/user/:name) or wildcard values
    // (/file/*pathname). Alternatively, use `Router::with_data(D)` and pass in arbitrary data for
    // routes to access and share using the `ctx.data()` method.
    let router = Router::new();

    // useful for JSON APIs
    #[derive(Deserialize, Serialize)]
    struct Account {
        id: u64,
        // ...
    }
    router
        .get_async("/account/:id", |_req, ctx| async move {
            if let Some(id) = ctx.param("id") {
                let accounts = ctx.kv("ACCOUNTS")?;
                return match accounts.get(id).json::<Account>().await? {
                    Some(account) => Response::from_json(&account),
                    None => Response::error("Not found", 404),
                };
            }

            Response::error("Bad Request", 400)
        })
        // handle files and fields from multipart/form-data requests
        .post_async("/upload", |mut req, _ctx| async move {
            let form = req.form_data().await?;
            if let Some(entry) = form.get("file") {
                match entry {
                    FormEntry::File(file) => {
                        let bytes = file.bytes().await?;
                    }
                    FormEntry::Field(_) => return Response::error("Bad Request", 400),
                }
                // ...

                if let Some(permissions) = form.get("permissions") {
                    // permissions == "a,b,c,d"
                }
                // or call `form.get_all("permissions")` if using multiple entries per field
            }

            Response::error("Bad Request", 400)
        })
        // read/write binary data
        .post_async("/echo-bytes", |mut req, _ctx| async move {
            let data = req.bytes().await?;
            if data.len() < 32 {
                return Response::error("Bad Request", 400);
            }

            Response::from_bytes(data)
        })
        .post_async("/proxy/universal", stream_proxy)
        .post_async("/azure-openai/completions", stream_proxy)
        .run(req, env)
        .await
}

async fn stream_proxy(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let data = req.bytes().await?;

    // Extract metadata for analytics
    let ip_address = req.headers().get("CF-Connecting-IP").ok().flatten();
    let country = req.headers().get("CF-IPCountry").ok().flatten();
    let cf_ray = req.headers().get("CF-Ray").ok().flatten();
    let domain = req.headers().get("Host").ok().flatten();
    // For deployment, we could use environment variables or default value
    let deployment = Some("cloudflare-worker".to_string());
    let env = ctx.env.clone();

    let xparams: ProxyUrlParams = match req.query() {
        Ok(v) => v,
        Err(e) => {
            console_error!("Query String Error: {}", e.to_string());

            return match Response::from_json(&json!({
                "error": true,
                "type": "Query String Error",
                "message": e.to_string(),
            })) {
                Ok(v) => Ok(v.with_status(400)),
                Err(e) => {
                    console_error!("Response Builder Error: {}", e.to_string());
                    Response::error("Internal Server Error!", 500)
                }
            };
        }
    };

    console_debug!("XParams: {xparams:?}");

    // let a = std::time::Instant::now();
    let data = match serde_json::from_slice::<AzureReqBodyStream>(&data) {
        Ok(stream_params) => {
            console_debug!("Stream Params: {stream_params:?}");
            if stream_params.stream == false {
                data
            } else {
                match std::str::from_utf8(&data) {
                    Ok(s) => {
                        #[cfg(debug_assertions)]
                        console_error!("ORIGINAL: {}", s);
                        // https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chatcompletionstreamoptions
                        // {"stream_options":{"include_usage": true}
                        // let trimmed = s.trim();
                        // let concat = format!("{}{}", &trimmed[..(trimmed.len() - 1)], r#","stream_options":{"include_usage": true}}"#);
                        let concat = format!(
                            "{}{}",
                            r#"{"stream_options":{"include_usage": true},"#,
                            &s.trim()[1..]
                        );
                        #[cfg(debug_assertions)]
                        console_error!("CONCAT: {concat}");
                        // #[cfg(debug_assertions)]
                        match serde_json::from_str::<serde_json::Value>(&concat) {
                            Ok(_) => {
                                console_log!("Parsed Ok!");
                            }
                            Err(e) => {
                                console_error!("Invalid JSON: {}", e);

                                return Response::error("Invalid UTF-8", 400);
                            }
                        }
                        // console_log!("=== Took {:?}", a.elapsed());
                        concat
                    }
                    Err(e) => {
                        console_error!("Invalid UTF-8: {}", e);
                        return Response::error("Invalid UTF-8", 400);
                    }
                }
                .as_str()
                .into()
            }
        }
        Err(e) => {
            console_error!("JSON Error: {}", e.to_string());
            return Response::error("Internal Server Error!!", 500);
        }
    };

    let proxy_headers = {
        static API_KEY_STR: &str = "api-key";
        static AUTH_KEY_STR: &str = "authorization";

        let mut proxy_headers = Headers::new();

        let (header_name, header_value) = match req.headers().get(API_KEY_STR) {
            Ok(Some(key)) => (API_KEY_STR, key),
            _ => match req.headers().get(AUTH_KEY_STR) {
                Ok(Some(key)) => (AUTH_KEY_STR, key),
                _ => {
                    console_error!("Request Error: Missing authorization headers");
                    return Response::error("Internal Server Error!!!", 500);
                }
            },
        };

        proxy_headers
            .set(header_name, &header_value)
            .expect("Should set a header value");

        proxy_headers
    };

    let proxy_url = xparams.u.clone();

    console_debug!("Proxy URL: {proxy_url}");

    let reqwester = reqwest::Client::new();
    let response = match reqwester
        .post(proxy_url)
        .headers(proxy_headers.into())
        .body(data)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            console_error!("Request Error: {}", e.to_string());
            return Response::error("Internal Server Error!!!!", 500);
        }
    };

    if response.status().is_success() {
        let mut my_response_headers = Headers::new();

        for (header_name, header_value) in response.headers() {
            if let Ok(value_str) = header_value.to_str() {
                my_response_headers
                    .append(header_name.as_str(), value_str)
                    .expect("Should set response header");
            }
        }

        // Set content type to match what's expected for streaming responses
        if !my_response_headers.has("content-type").unwrap_or(false) {
            my_response_headers
                .set("content-type", "text/event-stream")
                .expect("Should set content-type header");
        }

        // Add CORS headers if needed
        my_response_headers
            .set("Access-Control-Allow-Origin", "*")
            .expect("Should set CORS header");

        // Create a streaming response
        let status = response.status().as_u16();
        let (mut tx, rx) = futures_channel::mpsc::channel(10);

        // Spawn a task to process the incoming stream and send chunks to our channel
        wasm_bindgen_futures::spawn_local(async move {
            let mut stream = response.bytes_stream();

            while let Some(item) = stream.next().await {
                match item {
                    Ok(chunk) => {
                        // console_log!("Forwarding chunk of size: {}", chunk.len());
                        if tx.try_send(Ok(chunk.to_vec())).is_err() {
                            console_error!("Failed to forward chunk, receiver dropped");
                            break;
                        }
                        // worker::Delay::from(std::time::Duration::from_millis(100)).await;
                    }
                    Err(e) => {
                        console_error!("Error while streaming: {}", e);
                        let _ = tx.try_send(Err(Error::from(e.to_string())));
                        break;
                    }
                }
            }

            console_log!("Upstream stream completed with status {}", status);
        });

        // let mut temp_str: heapless::String<512> = heapless::String::new();
        let mut temp_str = String::new();

        // Capture analytics metadata for use in the stream closure
        let analytics_metadata = (
            xparams.app.clone(),
            xparams.ten_id.clone(),
            xparams.mod_id.clone(),
            xparams.ses_id.clone(),
            xparams.req_id.clone(),
            xparams.env_id.clone(),
            ip_address.clone(),
            country.clone(),
            cf_ray.clone(),
            domain.clone(),
            deployment.clone(),
            env.clone(),
        );

        // Create a ReadableStream from our channel receiver
        let stream = rx.map(move |result| {
            match result {
                Ok(bytes) => {
                    let chunk_str = unsafe{ std::str::from_utf8_unchecked(&bytes) };
                    if temp_str.len() > 0 {
                        console_log!("TEMP STRING LEN: {}", temp_str.len());
                        if let Some(pos) = chunk_str.find("\n") {
                            temp_str.push_str(&chunk_str[..pos])
                                //.expect("Failed to second push chunk")
                                ;
                        let choices_str = &temp_str;
                        console_debug!("TEMP STRING2: <!--\n{}\n-->", choices_str);

                        match serde_json::from_str::<StatsChunk>(choices_str) {
                            Ok(stats_chunk) => {
                                console_log!("STATS CHUNK A: <!--\n{:?}\n-->", stats_chunk);

                                // Collect analytics data
                                let analytics = UsageAnalytics::new(
                                    analytics_metadata.0.clone(), // app_id
                                    analytics_metadata.1.clone(), // tenant_id
                                    analytics_metadata.2.clone(), // module_id  
                                    analytics_metadata.3.clone(), // session_id
                                    analytics_metadata.4.clone(), // request_id
                                    analytics_metadata.5.clone(), // env_id
                                    analytics_metadata.6.clone(), // ip_address
                                    analytics_metadata.7.clone(), // country
                                    analytics_metadata.8.clone(), // cf_ray
                                    analytics_metadata.9.clone(), // domain
                                    analytics_metadata.10.clone(), // deployment
                                    stats_chunk.model.to_string(),
                                    stats_chunk.usage.prompt_tokens,
                                    stats_chunk.usage.completion_tokens,
                                    stats_chunk.usage.total_tokens,
                                );
                                
                                // Save analytics data asynchronously (fire-and-forget)
                                let env_clone = analytics_metadata.11.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    analytics.save(&env_clone).await;
                                });
                            }
                            Err(e) => {
                                console_error!("B: Failed to parse choices chunk: <!--\n{choices_str}\n-->\nError: {e}");
                            }
                        }
                        temp_str.clear();
                    }
                }

                if let Some(choices_position) = chunk_str.find(r#"{"choices":[]"#) {
                    console_debug!("CHOICES CHUNK: <!--\n{}\n-->", &chunk_str[choices_position..]);
                    if let Some(newline_position) = chunk_str.find("\n") {
                        let choices_str = &chunk_str[choices_position..newline_position];
                        console_debug!("CHOICES STRING: <!--\n{}\n-->", choices_str);
                        match serde_json::from_str::<StatsChunk>(choices_str) {
                            Ok(stats_chunk) => {
                                console_log!("STATS CHUNK B: <!--\n{:?}\n-->", stats_chunk);

                                // Collect analytics data
                                let analytics = UsageAnalytics::new(
                                    analytics_metadata.0.clone(), // app_id
                                    analytics_metadata.1.clone(), // tenant_id
                                    analytics_metadata.2.clone(), // module_id  
                                    analytics_metadata.3.clone(), // session_id
                                    analytics_metadata.4.clone(), // request_id
                                    analytics_metadata.5.clone(), // env_id
                                    analytics_metadata.6.clone(), // ip_address
                                    analytics_metadata.7.clone(), // country
                                    analytics_metadata.8.clone(), // cf_ray
                                    analytics_metadata.9.clone(), // domain
                                    analytics_metadata.10.clone(), // deployment
                                    stats_chunk.model.to_string(),
                                    stats_chunk.usage.prompt_tokens,
                                    stats_chunk.usage.completion_tokens,
                                    stats_chunk.usage.total_tokens,
                                );
                                
                                // Save analytics data asynchronously (fire-and-forget)
                                let env_clone = analytics_metadata.11.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    analytics.save(&env_clone).await;
                                });
                            }
                            Err(e) => {
                                console_error!("A: Failed to parse choices chunk:\nError: {:?}", e);
                            }
                        }
                    } else {
                        console_debug!(": CHOICES ELSE: NO ENTER IN STRING");
                        temp_str.clear();
                        temp_str.push_str(&chunk_str[choices_position..])
                            // .expect("Failed to push first chunk")
                            ;
                        console_debug!("TEMP STRING1: ----\n{}\n----", temp_str);
                    }
                    console_log!("CHUNK: ----\n{}\n----", chunk_str);
                }
                // console_log!("CHUNK: ----\n{}\n----", unsafe{ std::str::from_utf8_unchecked(&bytes) });
                Ok(bytes)
            },
            Err(e) => Err(Error::from(e.to_string())),
        }
    });

        // Return a streaming response
        match Response::from_stream(stream) {
            Ok(resp) => Ok(resp.with_headers(my_response_headers)),
            Err(e) => {
                console_error!("Error creating streaming response: {}", e);
                Response::error("Internal Server Error!!!!!", 500)
            }
        }
    } else {
        console_error!("Error {}", response.status());
        let status = response.status();
        let text = &response.text().await;
        Response::error(format!("{:?}", &text), status.into())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProxyUrlParams {
    pub app: String,
    pub u: String,
    pub env_id: Option<String>,
    pub ten_id: Option<String>,
    pub mod_id: Option<String>,
    pub ses_id: Option<String>,
    pub req_id: Option<String>,
    #[serde(rename = "api-version")]
    pub api_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AzureReqBodyStream {
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Deserialize)]
struct StatsChunk {
    pub model: HString<64>,
    pub usage: Usage,
}
#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    pub completion_tokens: u32,
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct AzurePartialResponseBody {
    pub id: HString<64>,
    pub created: u32,
    pub model: HString<64>,
    pub usage: Usage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_url_params_deserialization() {
        let json_str = r#"{
            "app": "test-app",
            "u": "https://api.openai.com/v1/chat/completions",
            "envId": "prod",
            "tenId": "tenant123",
            "modId": "module456",
            "sesId": "session789",
            "reqId": "request101",
            "api-version": "2023-05-15"
        }"#;

        let params: ProxyUrlParams = serde_json::from_str(json_str).unwrap();
        assert_eq!(params.app, "test-app");
        assert_eq!(params.u, "https://api.openai.com/v1/chat/completions");
        assert_eq!(params.env_id, Some("prod".to_string()));
        assert_eq!(params.ten_id, Some("tenant123".to_string()));
        assert_eq!(params.mod_id, Some("module456".to_string()));
        assert_eq!(params.ses_id, Some("session789".to_string()));
        assert_eq!(params.req_id, Some("request101".to_string()));
        assert_eq!(params.api_version, Some("2023-05-15".to_string()));
    }

    #[test]
    fn test_proxy_url_params_minimal() {
        let json_str = r#"{
            "app": "minimal-app",
            "u": "https://test.example.com"
        }"#;

        let params: ProxyUrlParams = serde_json::from_str(json_str).unwrap();
        assert_eq!(params.app, "minimal-app");
        assert_eq!(params.u, "https://test.example.com");
        assert_eq!(params.env_id, None);
        assert_eq!(params.ten_id, None);
        assert_eq!(params.mod_id, None);
        assert_eq!(params.ses_id, None);
        assert_eq!(params.req_id, None);
        assert_eq!(params.api_version, None);
    }

    #[test]
    fn test_azure_req_body_stream_defaults() {
        let json_str = r#"{}"#;
        let body: AzureReqBodyStream = serde_json::from_str(json_str).unwrap();
        assert_eq!(body.stream, false); // default value
    }

    #[test]
    fn test_azure_req_body_stream_explicit() {
        let json_str = r#"{"stream": true}"#;
        let body: AzureReqBodyStream = serde_json::from_str(json_str).unwrap();
        assert_eq!(body.stream, true);

        let json_str = r#"{"stream": false}"#;
        let body: AzureReqBodyStream = serde_json::from_str(json_str).unwrap();
        assert_eq!(body.stream, false);
    }

    #[test]
    fn test_usage_deserialization() {
        let json_str = r#"{
            "prompt_tokens": 150,
            "completion_tokens": 75,
            "total_tokens": 225
        }"#;

        let usage: Usage = serde_json::from_str(json_str).unwrap();
        assert_eq!(usage.prompt_tokens, 150);
        assert_eq!(usage.completion_tokens, 75);
        assert_eq!(usage.total_tokens, 225);
    }

    #[test]
    fn test_usage_with_default_completion_tokens() {
        let json_str = r#"{
            "prompt_tokens": 100,
            "total_tokens": 100
        }"#;

        let usage: Usage = serde_json::from_str(json_str).unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 0); // default value
        assert_eq!(usage.total_tokens, 100);
    }

    #[test]
    fn test_stats_chunk_deserialization() {
        let json_str = r#"{
            "model": "gpt-4",
            "usage": {
                "prompt_tokens": 200,
                "completion_tokens": 100,
                "total_tokens": 300
            }
        }"#;

        let stats: StatsChunk = serde_json::from_str(json_str).unwrap();
        assert_eq!(stats.model.as_str(), "gpt-4");
        assert_eq!(stats.usage.prompt_tokens, 200);
        assert_eq!(stats.usage.completion_tokens, 100);
        assert_eq!(stats.usage.total_tokens, 300);
    }

    #[test]
    fn test_azure_partial_response_body_deserialization() {
        let json_str = r#"{
            "id": "chatcmpl-123",
            "created": 1640995200,
            "model": "gpt-3.5-turbo",
            "usage": {
                "prompt_tokens": 50,
                "completion_tokens": 25,
                "total_tokens": 75
            }
        }"#;

        let response: AzurePartialResponseBody = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.id.as_str(), "chatcmpl-123");
        assert_eq!(response.created, 1640995200);
        assert_eq!(response.model.as_str(), "gpt-3.5-turbo");
        assert_eq!(response.usage.prompt_tokens, 50);
        assert_eq!(response.usage.completion_tokens, 25);
        assert_eq!(response.usage.total_tokens, 75);
    }

    #[test]
    fn test_json_parsing_error_handling() {
        // Test invalid JSON for ProxyUrlParams
        let invalid_json = r#"{"app": "test"}"#; // missing required 'u' field
        let result = serde_json::from_str::<ProxyUrlParams>(invalid_json);
        assert!(result.is_err());

        // Test invalid JSON for Usage
        let invalid_usage = r#"{"prompt_tokens": "not_a_number"}"#;
        let result = serde_json::from_str::<Usage>(invalid_usage);
        assert!(result.is_err());
    }

    #[test]
    fn test_heapless_string_limits() {
        // Test that HString<64> can handle strings up to 64 characters
        let long_model_name = "a".repeat(64);
        let json_str = format!(
            r#"{{"model": "{}", "usage": {{"prompt_tokens": 10, "total_tokens": 10}}}}"#,
            long_model_name
        );

        let stats = serde_json::from_str::<StatsChunk>(&json_str);
        assert!(stats.is_ok());

        // Test that strings longer than 64 characters should fail
        let too_long_model_name = "a".repeat(65);
        let json_str = format!(
            r#"{{"model": "{}", "usage": {{"prompt_tokens": 10, "total_tokens": 10}}}}"#,
            too_long_model_name
        );

        let stats = serde_json::from_str::<StatsChunk>(&json_str);
        assert!(stats.is_err());
    }

    #[test]
    fn test_complex_proxy_url_params_scenarios() {
        // Test with special characters in URL
        let json_str = r#"{
            "app": "test-app",
            "u": "https://api.openai.com/v1/chat/completions?model=gpt-4&stream=true",
            "envId": "prod-2024",
            "tenId": "tenant_123"
        }"#;

        let params: ProxyUrlParams = serde_json::from_str(json_str).unwrap();
        assert_eq!(params.app, "test-app");
        assert_eq!(
            params.u,
            "https://api.openai.com/v1/chat/completions?model=gpt-4&stream=true"
        );
        assert_eq!(params.env_id, Some("prod-2024".to_string()));
        assert_eq!(params.ten_id, Some("tenant_123".to_string()));
    }

    #[test]
    fn test_azure_req_body_with_extra_fields() {
        // Test that extra fields in JSON are ignored
        let json_str = r#"{
            "stream": true,
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "temperature": 0.7,
            "extra_field": "ignored"
        }"#;

        let body: AzureReqBodyStream = serde_json::from_str(json_str).unwrap();
        assert_eq!(body.stream, true);
    }

    #[test]
    fn test_usage_zero_values() {
        let json_str = r#"{
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        }"#;

        let usage: Usage = serde_json::from_str(json_str).unwrap();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn test_stats_chunk_with_minimal_model_name() {
        let json_str = r#"{
            "model": "a",
            "usage": {
                "prompt_tokens": 1,
                "total_tokens": 1
            }
        }"#;

        let stats: StatsChunk = serde_json::from_str(json_str).unwrap();
        assert_eq!(stats.model.as_str(), "a");
        assert_eq!(stats.usage.prompt_tokens, 1);
        assert_eq!(stats.usage.completion_tokens, 0); // default
        assert_eq!(stats.usage.total_tokens, 1);
    }

    #[test]
    fn test_azure_partial_response_edge_cases() {
        // Test with minimal valid values
        let json_str = r#"{
            "id": "1",
            "created": 0,
            "model": "m",
            "usage": {
                "prompt_tokens": 1,
                "total_tokens": 1
            }
        }"#;

        let response: AzurePartialResponseBody = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.id.as_str(), "1");
        assert_eq!(response.created, 0);
        assert_eq!(response.model.as_str(), "m");
        assert_eq!(response.usage.prompt_tokens, 1);
        assert_eq!(response.usage.completion_tokens, 0);
        assert_eq!(response.usage.total_tokens, 1);
    }

    #[test]
    fn test_malformed_json_handling() {
        // Test various malformed JSON strings
        let malformed_cases = vec![
            r#"{"app": "test""#,             // Unclosed JSON
            r#"{"app": test, "u": "url"}"#,  // Unquoted string
            r#"{"app": "test", "u": null}"#, // Null for required string field
            r#"{}"#,                         // Missing required fields
        ];

        for json_str in malformed_cases {
            let result = serde_json::from_str::<ProxyUrlParams>(json_str);
            assert!(
                result.is_err(),
                "Expected error for malformed JSON: {}",
                json_str
            );
        }
    }

    #[test]
    fn test_large_numeric_values() {
        // Test with maximum u32 values
        let json_str = format!(
            r#"{{
            "prompt_tokens": {},
            "completion_tokens": {},
            "total_tokens": {}
        }}"#,
            u32::MAX,
            u32::MAX,
            u32::MAX
        );

        let usage: Usage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(usage.prompt_tokens, u32::MAX);
        assert_eq!(usage.completion_tokens, u32::MAX);
        assert_eq!(usage.total_tokens, u32::MAX);
    }

    #[test]
    fn test_proxy_params_camel_case_conversion() {
        // Test that camelCase field names are properly converted
        let json_str = r#"{
            "app": "test",
            "u": "url",
            "envId": "env1",
            "tenId": "ten1", 
            "modId": "mod1",
            "sesId": "ses1",
            "reqId": "req1"
        }"#;

        let params: ProxyUrlParams = serde_json::from_str(json_str).unwrap();
        assert_eq!(params.env_id, Some("env1".to_string()));
        assert_eq!(params.ten_id, Some("ten1".to_string()));
        assert_eq!(params.mod_id, Some("mod1".to_string()));
        assert_eq!(params.ses_id, Some("ses1".to_string()));
        assert_eq!(params.req_id, Some("req1".to_string()));
    }
}
