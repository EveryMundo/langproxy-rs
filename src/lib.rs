use serde::{Deserialize, Serialize};
use serde_json::json;
// use hashbrown::HashMap;
use heapless::String as HString;
use futures_util::StreamExt;

use worker::*;

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
        .post_async("/azure-openai/completions", |mut req, _ctx| async move {
            let data = req.bytes().await?;
            
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
                            Response::error("Internal Server Error", 500)
                        }
                    };
                }
            };

            console_debug!("XParams: {xparams:?}");

            let stream_params: AzureReqBodyStream = match serde_json::from_slice(&data) {
                Ok(v) => v,
                Err(e) => {
                    console_error!("JSON Error: {}", e.to_string());
                    return Response::error("Internal Server Error", 500);
                }
            };
            
            console_debug!("Stream Params: {stream_params:?}");
            
            static API_KEY_STR: &str = "api-key";
            static AUTH_KEY_STR: &str = "authorization";
            
            let mut proxy_headers = Headers::new();
            
            let (header_name, header_value) = match req.headers().get(API_KEY_STR) {
                Ok(Some(key)) => (API_KEY_STR, key),
                _ => match req.headers().get(AUTH_KEY_STR) {
                    Ok(Some(key)) => (AUTH_KEY_STR, key),
                    _ => {
                        console_error!("Request Error: Missing authorization headers");
                        return Response::error("Internal Server Error!!", 500);
                    },
                }
            };

            proxy_headers.set(header_name, &header_value)
                .expect("Should set a header value");
            
            let proxy_url = xparams.u.clone();
            
            console_debug!("Proxy URL: {proxy_url}");

            let reqwester = reqwest::Client::new();
            let mut response = match reqwester.post(proxy_url)
                .headers(proxy_headers.into())
                .body(data)
                .send()
                .await {
                    Ok(res) => res,
                    Err(e) => {
                        console_error!("Request Error: {}", e.to_string());
                        return Response::error("Internal Server Error", 500);
                    }
                };

            /* match response.bytes().await {
                Ok(bytes ) => {
                    match Response::from_bytes(bytes.into()) {
                        Ok(r) => {
                            let mut my_response_headers = Headers::new();

                            for (header_name, header_value) in r.headers() {
                                my_response_headers.append(header_name.as_str(), header_value.as_str())
                                    .expect("Should set response header");
                            }

                            Ok(r.with_headers(my_response_headers))
                        }
                        Err(e) => {
                            console_error!("Error {}", e.to_string());
                            Response::error("Internal Server Error!!!", 502)
                        }
                    }
                },
                Err(e) => {
                    console_error!("Error {}", e.to_string());
                    return Response::error("Internal Server Error!!!", 501);
                }
            } */
            if response.status().is_success() {
                let mut my_response_headers = Headers::new();

                for (header_name, header_value) in response.headers() {
                    my_response_headers.append(header_name.as_str(), header_value.to_str().unwrap())
                        .expect("Should set response header");
                }

                let status = response.status().as_u16();
                let mut stream = response.bytes_stream();

                while let Some(item) = stream.next().await {
                    match item {
                        Ok(chunk) => {
                            // Process the chunk of bytes
                            // For example, print it or write it to a file
                            console_log!("Received chunk: {:?}\n\n", chunk);
                        }
                        Err(e) => {
                            console_error!("Error while streaming: {}", e);
                            // Handle the error, maybe break the loop or retry
                            break;
                        }
                    }
                };

                console_log!("Done {}", status);
                
                Response::from_bytes(b"Internal Server Error!!!".into())
            } else {
                console_error!("Error {}", response.status());
                Response::error("Internal Server Error!!!", response.status().into())
            }
        })
        .run(req, env).await
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
    