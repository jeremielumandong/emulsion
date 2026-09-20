//! Gemini's native image generation and prompt-guided image editing.
//! API: https://ai.google.dev/api/generate-content
use super::{Config, GenError, agent, check, decode, png};
use base64::prelude::{BASE64_STANDARD, Engine};
use image::RgbaImage;
use serde_json::{Value, json};

pub(super) const DEFAULT_MODEL: &str = "gemini-3.1-flash-image";

fn credentials(cfg: &Config) -> Result<(&str, &str), GenError> {
    let key = cfg
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| GenError::NotConfigured("add a Google Gemini API key in Settings".into()))?;
    let model = cfg
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_MODEL);
    let model = model.strip_prefix("models/").unwrap_or(model);
    if model.is_empty()
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(GenError::NotConfigured(
            "use a Gemini model ID, such as gemini-3.1-flash-image".into(),
        ));
    }
    Ok((key, model))
}

pub(super) fn reachable(cfg: &Config) -> Result<String, GenError> {
    let (key, model) = credentials(cfg)?;
    let mut response = agent()
        .get(format!("{}/models/{model}", cfg.endpoint()))
        .header("x-goog-api-key", key)
        .call()
        .map_err(|e| GenError::Http(e.to_string()))?;
    let bytes = check(&mut response)?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| GenError::Decode(e.to_string()))?;
    if let Some(methods) = value
        .get("supportedGenerationMethods")
        .and_then(Value::as_array)
        && !methods
            .iter()
            .any(|m| m.as_str() == Some("generateContent"))
    {
        return Err(GenError::Provider(format!(
            "{model} does not support generateContent"
        )));
    }
    Ok(value
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or(model)
        .to_string())
}

fn aspect_ratio(w: u32, h: u32) -> &'static str {
    // These ratios are shared by Gemini 2.5 and 3 image models. The caller
    // scales the result to the precise requested dimensions afterwards.
    let ratios = [
        ("1:1", 1., 1.),
        ("2:3", 2., 3.),
        ("3:2", 3., 2.),
        ("3:4", 3., 4.),
        ("4:3", 4., 3.),
        ("4:5", 4., 5.),
        ("5:4", 5., 4.),
        ("9:16", 9., 16.),
        ("16:9", 16., 9.),
        ("21:9", 21., 9.),
    ];
    let target = w.max(1) as f64 / h.max(1) as f64;
    ratios
        .into_iter()
        .min_by(|a, b| {
            (target / (a.1 / a.2))
                .ln()
                .abs()
                .total_cmp(&(target / (b.1 / b.2)).ln().abs())
        })
        .unwrap()
        .0
}

fn image_part(image: &RgbaImage) -> Value {
    json!({"inlineData": {"mimeType": "image/png", "data": BASE64_STANDARD.encode(png(image))}})
}

#[allow(clippy::too_many_arguments)]
pub(super) fn request(
    cfg: &Config,
    prompt: &str,
    negative: Option<&str>,
    image: Option<&RgbaImage>,
    mask: Option<&RgbaImage>,
    w: u32,
    h: u32,
) -> Result<RgbaImage, GenError> {
    let (key, model) = credentials(cfg)?;
    if mask.is_some() && image.is_none() {
        return Err(GenError::Provider(
            "an edit mask requires a source image".into(),
        ));
    }
    let mut instructions = format!("Return one finished image. {prompt}");
    if let Some(negative) = negative.map(str::trim).filter(|s| !s.is_empty()) {
        instructions.push_str(&format!("\nAvoid these elements: {negative}"));
    }
    if mask.is_some() {
        instructions.push_str("\nThe first attached image is the source artwork. The second is an aligned edit mask: white marks the area to replace, black marks the area to preserve. Apply the requested change inside the white area, matching the source lighting, style and edges. Return the complete edited source image with its original framing and object positions. Do not draw the mask, labels, or a side-by-side comparison.");
    } else if image.is_some() {
        instructions.push_str("\nEdit the attached source image according to the request; preserve its framing and unrelated details.");
    }
    let mut parts = vec![json!({"text": instructions})];
    if let Some(image) = image {
        parts.push(image_part(image));
    }
    if let Some(mask) = mask {
        parts.push(image_part(mask));
    }
    let mut image_config = json!({"aspectRatio": aspect_ratio(w, h)});
    // Gemini 2.5 does not accept imageSize; newer image models support 1K.
    if !model.starts_with("gemini-2.") {
        image_config["imageSize"] = json!("1K");
    }
    let body = json!({
        "contents": [{"role": "user", "parts": parts}],
        "generationConfig": {"responseModalities": ["TEXT", "IMAGE"], "imageConfig": image_config}
    });
    let mut response = agent()
        .post(format!("{}/models/{model}:generateContent", cfg.endpoint()))
        .header("x-goog-api-key", key)
        .send_json(body)
        .map_err(|e| GenError::Http(e.to_string()))?;
    let bytes = check(&mut response)?;
    parse_image(&bytes)
}

fn parse_image(bytes: &[u8]) -> Result<RgbaImage, GenError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| GenError::Decode(e.to_string()))?;
    if let Some(reason) = value
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
        && reason != "BLOCK_REASON_UNSPECIFIED"
    {
        return Err(GenError::Provider(format!(
            "Gemini blocked this request ({reason})"
        )));
    }
    let candidates = value.get("candidates").and_then(Value::as_array);
    let mut explanation = String::new();
    for candidate in candidates.into_iter().flatten() {
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            explanation.push_str(reason);
            explanation.push_str(": ");
        }
        for part in candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // Some image models return intermediate thinking images as well.
            if part.get("thought").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if let Some(data) = part.get("inlineData").or_else(|| part.get("inline_data")) {
                let mime = data
                    .get("mimeType")
                    .or_else(|| data.get("mime_type"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !mime.starts_with("image/") {
                    return Err(GenError::Decode(
                        "Gemini returned inline data without an image MIME type".into(),
                    ));
                }
                let encoded = data
                    .get("data")
                    .and_then(Value::as_str)
                    .ok_or_else(|| GenError::Decode("Gemini's image data is missing".into()))?;
                let raw = BASE64_STANDARD
                    .decode(encoded)
                    .map_err(|e| GenError::Decode(format!("invalid Gemini image base64: {e}")))?;
                return decode(&raw);
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                explanation.extend(text.chars().take(300));
            }
        }
    }
    let explanation: String = explanation.chars().take(400).collect();
    Err(GenError::Provider(if explanation.is_empty() {
        "Gemini returned no image; choose a Gemini image-generation model".into()
    } else {
        format!("Gemini returned no image: {explanation}")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::Provider;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn server(reply: Value) -> (Config, thread::JoinHandle<(String, Value)>) {
        server_status(reply, "200 OK")
    }

    fn server_status(
        reply: Value,
        status: &'static str,
    ) -> (Config, thread::JoinHandle<(String, Value)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1beta", listener.local_addr().unwrap());
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut headers = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(!line.is_empty(), "request ended before headers");
                if line == "\r\n" {
                    break;
                }
                if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse::<usize>().unwrap();
                }
                headers.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let value = if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&body).unwrap()
            };
            let bytes = serde_json::to_vec(&reply).unwrap();
            write!(stream, "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", bytes.len()).unwrap();
            stream.write_all(&bytes).unwrap();
            (headers, value)
        });
        (
            Config {
                provider: Provider::Google,
                endpoint: Some(endpoint),
                model: None,
                api_key: Some("test-key".into()),
            },
            task,
        )
    }

    #[test]
    fn google_reachable_only_fetches_model_metadata() {
        let (cfg, task) = server(
            json!({"displayName":"Gemini Image", "supportedGenerationMethods":["generateContent"]}),
        );
        assert_eq!(reachable(&cfg).unwrap(), "Gemini Image");
        let (headers, body) = task.join().unwrap();
        assert!(headers.starts_with(&format!("GET /v1beta/models/{DEFAULT_MODEL} HTTP/1.1")));
        assert!(headers.to_lowercase().contains("x-goog-api-key: test-key"));
        assert!(body.is_null());
    }

    #[test]
    fn google_edit_sends_source_and_separate_mask_and_uses_final_image() {
        let source = RgbaImage::from_pixel(4, 3, image::Rgba([30, 40, 50, 255]));
        let mask = RgbaImage::from_pixel(4, 3, image::Rgba([255; 4]));
        let final_image = RgbaImage::from_pixel(4, 3, image::Rgba([60, 70, 80, 255]));
        let mut thought = image_part(&source);
        thought["thought"] = json!(true);
        let (cfg, task) = server(
            json!({"candidates":[{"content":{"parts":[thought, image_part(&final_image)]},"finishReason":"STOP"}]}),
        );
        assert_eq!(
            request(
                &cfg,
                "paint a tree",
                Some("letters"),
                Some(&source),
                Some(&mask),
                400,
                300
            )
            .unwrap(),
            final_image
        );
        let (headers, body) = task.join().unwrap();
        assert!(headers.starts_with(&format!(
            "POST /v1beta/models/{DEFAULT_MODEL}:generateContent HTTP/1.1"
        )));
        let parts = body["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[1], image_part(&source));
        assert_eq!(parts[2], image_part(&mask));
        assert!(
            parts[0]["text"]
                .as_str()
                .unwrap()
                .contains("white marks the area to replace")
        );
        assert!(
            parts[0]["text"]
                .as_str()
                .unwrap()
                .contains("Avoid these elements: letters")
        );
        assert_eq!(
            body["generationConfig"]["imageConfig"]["aspectRatio"],
            "4:3"
        );
    }

    #[test]
    fn google_blocked_and_malformed_results_are_errors() {
        let blocked = br#"{"promptFeedback":{"blockReason":"SAFETY"}}"#;
        assert!(matches!(parse_image(blocked), Err(GenError::Provider(s)) if s.contains("SAFETY")));
        let text = br#"{"candidates":[{"finishReason":"SAFETY","content":{"parts":[{"text":"Cannot create image"}]}}]}"#;
        assert!(
            matches!(parse_image(text), Err(GenError::Provider(s)) if s.contains("Cannot create image"))
        );
        let bad = br#"{"candidates":[{"content":{"parts":[{"inlineData":{"mimeType":"image/png","data":"not base64!"}}]}}]}"#;
        assert!(matches!(parse_image(bad), Err(GenError::Decode(_))));
    }

    #[test]
    fn google_fill_preserves_source_and_clips_to_original_selection() {
        use emulsion_raster::{Mask, Raster};

        let (w, h) = (48, 40);
        let source = Raster::from_srgba8(w, h, &[30, 40, 50, 255].repeat((w * h) as usize));
        let before = source.to_srgba8();
        let coverage: Vec<u8> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if (8..36).contains(&x) && (8..32).contains(&y) {
                    if (19..25).contains(&x) && (17..23).contains(&y) {
                        0 // Interior hole must remain untouched after feathering.
                    } else if x < 12 {
                        96 // Preserve partial selection coverage as well.
                    } else {
                        255
                    }
                } else {
                    0
                }
            })
            .collect();
        let mask = Mask::from_pixels(w, h, 0, &coverage);
        let generated = RgbaImage::from_pixel(64, 64, image::Rgba([180, 90, 20, 255]));
        let (cfg, task) = server(json!({"candidates":[{
            "content":{"parts":[image_part(&generated)]}, "finishReason":"STOP"
        }]}));
        let (layer, rect) = crate::generate::fill(
            &cfg,
            &source,
            &mask,
            "add warm paint",
            None,
            &crate::jobs::Job::new(),
        )
        .unwrap();
        task.join().unwrap();
        assert_eq!(rect, source.bounds());
        assert_eq!(
            source.to_srgba8(),
            before,
            "fill must not alter the source raster"
        );
        let result = layer.to_srgba8();
        for (pixel, selected) in result.as_chunks::<4>().0.iter().zip(&coverage) {
            if *selected == 0 {
                assert_eq!(pixel[3], 0, "generated pixels escaped the selection");
            }
            assert!(
                pixel[3] <= *selected,
                "partial selection became more opaque"
            );
        }
        assert!(
            result
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 200 && p[0] > 150),
            "selected pixels must contain the generated color"
        );
    }

    #[test]
    fn google_http_error_reports_nested_provider_message() {
        let (cfg, task) = server_status(
            json!({"error":{"code":429,"message":"Image quota exhausted","status":"RESOURCE_EXHAUSTED"}}),
            "429 Too Many Requests",
        );
        let error = reachable(&cfg).unwrap_err();
        task.join().unwrap();
        assert!(
            matches!(error, GenError::Provider(s) if s.contains("429") && s.contains("Image quota exhausted"))
        );
    }
}
