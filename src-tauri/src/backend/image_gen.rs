//! backend/image_gen —— 文生图统一调度引擎（P2 级能力）。
//!
//! 适配三大主流文生图通道：
//! 1. NovelAI Image API（二次元模型首选，走现有 NovelAI Token 通道）
//! 2. Stable Diffusion WebUI（A1111 / Forge / SD.Next）：/sdapi/v1/txt2img
//! 3. ComfyUI API：通过 /prompt 提交图并等待 /history 输出
//!
//! 生成图像保存至 <data_root>/default-user/user/images/，并返回 /user-images/<filename>
//! 相对路径，由现有的静态资源端点（assets.rs）直接提供服务。

use super::providers::{json_resp, read_secret, validate_egress_url, EgressPolicy};
use super::BackendState;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const NOVELAI_IMAGE_URL: &str = "https://image.novelai.net/ai/generate-image";
const DEFAULT_SD_ENDPOINT: &str = "http://127.0.0.1:7860";
const DEFAULT_COMFY_ENDPOINT: &str = "http://127.0.0.1:8188";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ImageGenParams {
    pub provider: Option<String>,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub steps: Option<u32>,
    pub cfg_scale: Option<f32>,
    pub sampler: Option<String>,
    pub endpoint: Option<String>,
    pub token: Option<String>,
    pub model: Option<String>,
}

/// 统一生图入口 POST /api/images/generate
pub async fn generate(
    st: &BackendState,
    body: &Value,
    origin: Option<HeaderValue>,
) -> Response {
    let params: ImageGenParams = match serde_json::from_value(body.clone()) {
        Ok(p) => p,
        Err(e) => {
            return json_resp(
                StatusCode::BAD_REQUEST,
                json!({ "error": { "message": format!("生图参数格式不合法: {e}") } }),
                origin,
            );
        }
    };

    if params.prompt.trim().is_empty() {
        return json_resp(
            StatusCode::BAD_REQUEST,
            json!({ "error": { "message": "提示词 prompt 不能为空" } }),
            origin,
        );
    }

    let provider = params
        .provider
        .as_deref()
        .unwrap_or("novelai")
        .to_ascii_lowercase();

    let image_bytes_res = match provider.as_str() {
        "novelai" | "nai" => generate_novelai(st, &params).await,
        "sd" | "stablediffusion" | "sdwebui" => generate_sd(st, &params).await,
        "comfyui" | "comfy" => generate_comfyui(st, &params).await,
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("不支持的生图提供商: {other}"),
        )),
    };

    let image_bytes = match image_bytes_res {
        Ok(b) => b,
        Err((code, msg)) => {
            log::warn!(target: super::logx::EXT, "生图失败 ({}): {}", code, msg);
            return json_resp(code, json!({ "error": { "message": msg } }), origin);
        }
    };

    // 写入 <data_root>/default-user/user/images/
    let user_images_dir = st.data_root.join("default-user").join("user").join("images");
    if let Err(e) = tokio::fs::create_dir_all(&user_images_dir).await {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": { "message": format!("创建图片存储目录失败: {e}") } }),
            origin,
        );
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut rand_buf = [0u8; 4];
    let _ = getrandom::getrandom(&mut rand_buf);
    let rand_hex = hex_encode(&rand_buf);
    let filename = format!("img_{}_{}.png", now, rand_hex);
    let target_path = user_images_dir.join(&filename);

    if let Err(e) = tokio::fs::write(&target_path, &image_bytes).await {
        return json_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": { "message": format!("写入图片文件失败: {e}") } }),
            origin,
        );
    }

    log::info!(
        target: super::logx::EXT,
        "生图完成 provider={} filename={} bytes={}",
        provider,
        filename,
        image_bytes.len()
    );

    let url = format!("/user-images/{}", filename);
    json_resp(
        StatusCode::OK,
        json!({
            "ok": true,
            "url": url,
            "filename": filename,
            "provider": provider
        }),
        origin,
    )
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// NovelAI 生图流程
async fn generate_novelai(
    st: &BackendState,
    params: &ImageGenParams,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let token = params
        .token
        .clone()
        .or_else(|| read_secret(st, "novelai"))
        .or_else(|| read_secret(st, "api_key_novelai"))
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "缺少 NovelAI Token，请在设置中配置或在请求中传入".into(),
            )
        })?;

    let model = params
        .model
        .as_deref()
        .unwrap_or("nai-diffusion-3");

    let neg = params.negative_prompt.as_deref().unwrap_or(
        "lowres, {bad}, error, fewer, extra, missing, worst quality, jpeg artifacts, bad anatomy, bad hands, text, watermark",
    );

    let payload = json!({
        "input": params.prompt,
        "model": model,
        "action": "generate",
        "parameters": {
            "width": params.width.unwrap_or(832),
            "height": params.height.unwrap_or(1216),
            "scale": params.cfg_scale.unwrap_or(5.0),
            "sampler": params.sampler.as_deref().unwrap_or("k_euler"),
            "steps": params.steps.unwrap_or(28),
            "n_samples": 1,
            "ucPreset": 0,
            "qualityToggle": true,
            "dynamic_thresholding": false,
            "negative_prompt": neg
        }
    });

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("创建 HTTP 客户端失败: {e}")))?;

    let res = client
        .post(NOVELAI_IMAGE_URL)
        .header("Authorization", format!("Bearer {}", token.trim()))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("连接 NovelAI 图像接口失败: {e}")))?;

    let status = res.status();
    if !status.is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("NovelAI 生图返回错误 ({status}): {err_text}"),
        ));
    }

    let bytes = res
        .bytes()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("读取 NovelAI 图像数据流失败: {e}")))?;

    // NovelAI 返回包含 image_0.png 的 zip 包或直接 PNG 二进制
    if bytes.starts_with(b"\x89PNG") {
        return Ok(bytes.to_vec());
    }

    // 尝试以 zip 解压第一张图
    match zip::ZipArchive::new(std::io::Cursor::new(&bytes)) {
        Ok(mut archive) => {
            for i in 0..archive.len() {
                if let Ok(mut file) = archive.by_index(i) {
                    if file.name().ends_with(".png") || file.name().ends_with(".jpg") {
                        let mut buf = Vec::new();
                        if std::io::copy(&mut file, &mut buf).is_ok() {
                            return Ok(buf);
                        }
                    }
                }
            }
            Err((
                StatusCode::BAD_GATEWAY,
                "NovelAI Zip 包内未找到有效的图像文件".into(),
            ))
        }
        Err(_) => {
            // 若不是 zip 也不是 PNG 签名，按原数据返回尝试
            Ok(bytes.to_vec())
        }
    }
}

/// Stable Diffusion WebUI (/sdapi/v1/txt2img) 生图流程
async fn generate_sd(
    _st: &BackendState,
    params: &ImageGenParams,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let ep_raw = params
        .endpoint
        .as_deref()
        .unwrap_or(DEFAULT_SD_ENDPOINT);
    let valid_ep = validate_egress_url(ep_raw, EgressPolicy::SelfHosted)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("SD 接口地址不合法: {e}")))?;

    let url = format!("{}/sdapi/v1/txt2img", valid_ep.trim_end_matches('/'));

    let neg = params.negative_prompt.as_deref().unwrap_or(
        "lowres, bad anatomy, bad hands, text, error, missing fingers, extra digit, fewer digits, cropped, worst quality, low quality, normal quality, jpeg artifacts, signature, watermark, username, blurry",
    );

    let payload = json!({
        "prompt": params.prompt,
        "negative_prompt": neg,
        "steps": params.steps.unwrap_or(20),
        "width": params.width.unwrap_or(512),
        "height": params.height.unwrap_or(768),
        "cfg_scale": params.cfg_scale.unwrap_or(7.0),
        "sampler_name": params.sampler.as_deref().unwrap_or("Euler a")
    });

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("创建 HTTP 客户端失败: {e}")))?;

    let res = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("连接 SD WebUI 接口失败: {e}")))?;

    let status = res.status();
    if !status.is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("SD WebUI 返回错误 ({status}): {err_text}"),
        ));
    }

    let json_val: Value = res
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("解析 SD WebUI 响应 JSON 失败: {e}")))?;

    let b64_img = json_val
        .get("images")
        .and_then(|imgs| imgs.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                "SD WebUI 响应中未找到有效的 images 字段".into(),
            )
        })?;

    let clean_b64 = b64_img.split(',').last().unwrap_or(b64_img);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(clean_b64.trim())
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Base64 解码生图数据失败: {e}")))?;

    Ok(bytes)
}

/// ComfyUI 生图流程 (简易 txt2img graph 或直接 prompt 提交)
async fn generate_comfyui(
    _st: &BackendState,
    params: &ImageGenParams,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let ep_raw = params
        .endpoint
        .as_deref()
        .unwrap_or(DEFAULT_COMFY_ENDPOINT);
    let valid_ep = validate_egress_url(ep_raw, EgressPolicy::SelfHosted)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("ComfyUI 接口地址不合法: {e}")))?;

    let base_url = valid_ep.trim_end_matches('/');
    let prompt_url = format!("{}/prompt", base_url);

    // 标准简易 ComfyUI txt2img 节点图
    let prompt_graph = json!({
        "client_id": "stoc_image_client",
        "prompt": {
            "3": {
                "class_type": "KSampler",
                "inputs": {
                    "cfg": params.cfg_scale.unwrap_or(7.0),
                    "denoise": 1,
                    "latent_image": ["5", 0],
                    "model": ["4", 0],
                    "negative": ["7", 0],
                    "positive": ["6", 0],
                    "sampler_name": params.sampler.as_deref().unwrap_or("euler"),
                    "scheduler": "normal",
                    "seed": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() % 1000000000,
                    "steps": params.steps.unwrap_or(20)
                }
            },
            "4": {
                "class_type": "CheckpointLoaderSimple",
                "inputs": {
                    "ckpt_name": params.model.as_deref().unwrap_or("v1-5-pruned-emaonly.safetensors")
                }
            },
            "5": {
                "class_type": "EmptyLatentImage",
                "inputs": {
                    "batch_size": 1,
                    "height": params.height.unwrap_or(768),
                    "width": params.width.unwrap_or(512)
                }
            },
            "6": {
                "class_type": "CLIPTextEncode",
                "inputs": {
                    "clip": ["4", 1],
                    "text": params.prompt
                }
            },
            "7": {
                "class_type": "CLIPTextEncode",
                "inputs": {
                    "clip": ["4", 1],
                    "text": params.negative_prompt.as_deref().unwrap_or("low quality, bad anatomy")
                }
            },
            "8": {
                "class_type": "VAEDecode",
                "inputs": {
                    "samples": ["3", 0],
                    "vae": ["4", 2]
                }
            },
            "9": {
                "class_type": "SaveImage",
                "inputs": {
                    "filename_prefix": "STOC",
                    "images": ["8", 0]
                }
            }
        }
    });

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("创建 HTTP 客户端失败: {e}")))?;

    let submit_res = client
        .post(&prompt_url)
        .json(&prompt_graph)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("提交 ComfyUI 任务失败: {e}")))?;

    if !submit_res.status().is_success() {
        let err_text = submit_res.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("ComfyUI 任务提交被拒绝: {err_text}"),
        ));
    }

    let submit_json: Value = submit_res.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("解析 ComfyUI 提交响应失败: {e}"),
        )
    })?;

    let prompt_id = submit_json
        .get("prompt_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                "ComfyUI 未返回有效 prompt_id".into(),
            )
        })?;

    // 轮询 /history/{prompt_id} 最长等待 120 秒
    let history_url = format!("{}/history/{}", base_url, prompt_id);
    let mut image_info: Option<(String, String, String)> = None;

    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if let Ok(hist_res) = client.get(&history_url).send().await {
            if let Ok(hist_json) = hist_res.json::<Value>().await {
                if let Some(task) = hist_json.get(prompt_id) {
                    if let Some(outputs) = task.get("outputs") {
                        if let Some(node9) = outputs.get("9").or_else(|| outputs.as_object().and_then(|m| m.values().next())) {
                            if let Some(images) = node9.get("images").and_then(|arr| arr.as_array()) {
                                if let Some(img0) = images.first() {
                                    let filename = img0.get("filename").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let subfolder = img0.get("subfolder").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let img_type = img0.get("type").and_then(|v| v.as_str()).unwrap_or("output").to_string();
                                    if !filename.is_empty() {
                                        image_info = Some((filename, subfolder, img_type));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let (filename, subfolder, img_type) = image_info.ok_or_else(|| {
        (
            StatusCode::GATEWAY_TIMEOUT,
            "ComfyUI 生成超时或未产出图片".into(),
        )
    })?;

    // 下载生成的图片
    let view_url = format!(
        "{}/view?filename={}&subfolder={}&type={}",
        base_url, filename, subfolder, img_type
    );
    let img_res = client
        .get(&view_url)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("下载 ComfyUI 图像失败: {e}")))?;

    let bytes = img_res
        .bytes()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("读取 ComfyUI 图像数据失败: {e}")))?;

    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xaa, 0xff]), "000faaff");
    }

    #[test]
    fn test_params_serialization() {
        let json_str = r#"{
            "prompt": "1girl, smiling",
            "provider": "sd",
            "width": 512,
            "height": 768
        }"#;
        let parsed: Result<ImageGenParams, _> = serde_json::from_str(json_str);
        assert!(parsed.is_ok());
        let p = parsed.unwrap();
        assert_eq!(p.prompt, "1girl, smiling");
        assert_eq!(p.provider.as_deref(), Some("sd"));
        assert_eq!(p.width, Some(512));
        assert_eq!(p.height, Some(768));
    }

    fn st_with(tag: &str) -> super::super::test_support::TestBackend {
        let tmp = super::super::test_support::TestBackend::temp(&format!("stoc-imagegen-{tag}-"));
        let root = tmp.path().to_path_buf();
        super::super::test_support::TestBackend::new(tmp, BackendState {
            token: std::sync::Arc::new("t".into()),
            data_root: root,
            max_body_bytes: 1024,
            settings_svc: std::sync::Arc::new(super::super::settings::SettingsService::new()),
            chats_svc: std::sync::Arc::new(super::super::chats::ChatsService::new()),
            maid_tokens: std::sync::Arc::new(super::super::extras::MaidTokens::new()),
            vault: std::sync::Arc::new(super::super::vault::Vault::new_memory()),
        })
    }

    #[tokio::test]
    async fn test_empty_prompt_rejected() {
        let st = st_with("empty");
        let body = json!({ "prompt": "   " });
        let resp = generate(&st, &body, None).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
