// Copyright (c) 2025 EveryMundo
// All rights reserved.

use serde::{Deserialize, Serialize};
use serde_json::json;
// use hashbrown::HashMap;
use heapless::String as HString;
use futures_util::StreamExt;

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
        .run(req, env).await
}

async fn stream_proxy (mut req: Request, ctx: RouteContext<()>) -> Result <Response> {
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
            if stream_params.stream == false { data }
            else {
                match std::str::from_utf8(&data) {
                    Ok(s) => {
                        #[cfg(debug_assertions)]
                        console_error!("ORIGINAL: {}", s);
                        // https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chatcompletionstreamoptions
                        // {"stream_options":{"include_usage": true}
                        // let trimmed = s.trim();
                        // let concat = format!("{}{}", &trimmed[..(trimmed.len() - 1)], r#","stream_options":{"include_usage": true}}"#);
                        let concat = format!("{}{}", r#"{"stream_options":{"include_usage": true},"#, &s.trim()[1..]);
                        #[cfg(debug_assertions)]
                        console_error!("CONCAT: {concat}");
                        // #[cfg(debug_assertions)]
                        match serde_json::from_str::<serde_json::Value>(&concat) {
                            Ok(_) => { console_log!("Parsed Ok!"); }
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
                }.as_str().into()
            }
        },
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
                },
            }
        };

        proxy_headers.set(header_name, &header_value)
            .expect("Should set a header value");

        proxy_headers
    };

    let proxy_url = xparams.u.clone();

    console_debug!("Proxy URL: {proxy_url}");

    let reqwester = reqwest::Client::new();
    let response = match reqwester.post(proxy_url)
        .headers(proxy_headers.into())
        .body(data)
        .send()
        .await {
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
                my_response_headers.append(header_name.as_str(), value_str)
                    .expect("Should set response header");
            }
        }

        // Set content type to match what's expected for streaming responses
        if !my_response_headers.has("content-type").unwrap_or(false) {
            my_response_headers.set("content-type", "text/event-stream")
                .expect("Should set content-type header");
        }

        // Add CORS headers if needed
        my_response_headers.set("Access-Control-Allow-Origin", "*")
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
        let text= &response.text().await;
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
