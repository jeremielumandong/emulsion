//! OpenAI Images API: explicit generation/edit requests and an unbilled model
//! lookup for connection checks. Schema checked against the official Images
//! generate/edit references on 2026-09-19; edits accept JSON image data URLs.

use super::{Config, GenError, agent, check, decode, png};
use base64::prelude::{BASE64_STANDARD, Engine};
use image::{Rgba, RgbaImage};
use serde_json::{Value, json};

pub(super) const DEFAULT_MODEL: &str = "gpt-image-2.5-sunburst";

fn api_key(cfg: &Config) -> Result<&str, GenError> {
    let key = cfg
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| {
            GenError::NotConfigured("add an OpenAI API key in Settings › Image generation".into())
        })?;
    if key.contains(['\r', '\n']) {
        return Err(GenError::NotConfigured(
            "the OpenAI API key contains a line break".into(),
        ));
    }
    Ok(key)
}

fn model(cfg: &Config) -> &str {
    cfg.model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or(DEFAULT_MODEL)
}

fn path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// Check authentication/model access without creating a billable image.
pub(super) fn reachable(cfg: &Config) -> Result<String, GenError> {
    let key = api_key(cfg)?;
    let mut response = agent()
        .get(format!(
            "{}/models/{}",
            cfg.endpoint(),
            path_segment(model(cfg))
        ))
        .header("Authorization", format!("Bearer {key}"))
        .call()
        .map_err(|e| GenError::Http(e.to_string()))?;
    let body: Value = serde_json::from_slice(&check(&mut response)?)
        .map_err(|e| GenError::Decode(e.to_string()))?;
    body.get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| GenError::Decode("no model id in the OpenAI reply".into()))
}

/// These standard sizes are supported across GPT Image model versions. The
/// caller fits the result to the document/crop after receiving the image.
fn size(w: u32, h: u32) -> &'static str {
    let ratio = w as f64 / h as f64;
    if ratio > 1.2 {
        "1536x1024"
    } else if ratio < 1.0 / 1.2 {
        "1024x1536"
    } else {
        "1024x1024"
    }
}

fn data_url(image: &RgbaImage) -> Result<String, GenError> {
    let encoded = format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(png(image))
    );
    if encoded.len() > 20_971_520 {
        return Err(GenError::Provider(
            "the image input exceeds OpenAI's 20 MB data URL limit".into(),
        ));
    }
    Ok(encoded)
}

/// Emulsion marks editable pixels white; OpenAI marks them transparent.
fn edit_mask(mask: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(mask.width(), mask.height(), |x, y| {
        let editable = mask.get_pixel(x, y)[0];
        Rgba([255, 255, 255, 255 - editable])
    })
}

pub(super) fn request(
    cfg: &Config,
    prompt: &str,
    negative: Option<&str>,
    image: Option<&RgbaImage>,
    mask: Option<&RgbaImage>,
    w: u32,
    h: u32,
) -> Result<RgbaImage, GenError> {
    let key = api_key(cfg)?;
    if prompt.trim().is_empty() || w == 0 || h == 0 {
        return Err(GenError::Provider(
            "image generation needs a prompt and nonzero dimensions".into(),
        ));
    }
    if mask.is_some() && image.is_none() {
        return Err(GenError::Provider(
            "an edit mask requires an input image".into(),
        ));
    }
    if let Some(image) = image {
        if image.width() == 0 || image.height() == 0 {
            return Err(GenError::Provider("the input image is empty".into()));
        }
        if mask.is_some_and(|mask| mask.dimensions() != image.dimensions()) {
            return Err(GenError::Provider(
                "the input image and edit mask must have identical dimensions".into(),
            ));
        }
    }
    // GPT Image has no negative_prompt field; retain those constraints in the
    // actual prompt rather than sending unsupported provider parameters.
    let prompt = match negative.map(str::trim).filter(|text| !text.is_empty()) {
        Some(negative) => format!("{prompt}\n\nAvoid these elements: {negative}"),
        None => prompt.to_string(),
    };
    let mut body = json!({
        "model": model(cfg), "prompt": prompt, "n": 1,
        "size": size(w, h), "output_format": "png",
    });
    let endpoint = if let Some(image) = image {
        body["images"] = json!([{"image_url": data_url(image)?}]);
        if let Some(mask) = mask {
            body["mask"] = json!({"image_url": data_url(&edit_mask(mask))?});
        }
        "edits"
    } else {
        "generations"
    };
    let mut response = agent()
        .post(format!("{}/images/{endpoint}", cfg.endpoint()))
        .header("Authorization", format!("Bearer {key}"))
        .send_json(body)
        .map_err(|e| GenError::Http(e.to_string()))?;
    let body: Value = serde_json::from_slice(&check(&mut response)?)
        .map_err(|e| GenError::Decode(e.to_string()))?;
    let encoded = body
        .pointer("/data/0/b64_json")
        .and_then(Value::as_str)
        .filter(|image| !image.is_empty())
        .ok_or_else(|| GenError::Decode("no base64 image in the OpenAI reply".into()))?;
    let bytes = BASE64_STANDARD
        .decode(encoded)
        .map_err(|e| GenError::Decode(e.to_string()))?;
    decode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::time::{Duration, Instant};

    struct Recorded {
        request: String,
        authorization: String,
        body: Value,
    }

    fn mock(status: u16, answer: Value) -> (Config, std::thread::JoinHandle<Recorded>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            let start = Instant::now();
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed() < Duration::from_secs(10) =>
                    {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("mock did not receive a request: {error}"),
                }
            };
            // Accepted sockets can inherit nonblocking mode on macOS.
            // Buffered request reads need blocking mode plus a bounded timeout.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut authorization = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if lower.starts_with("authorization:") {
                    authorization = line.split_once(':').unwrap().1.trim().to_string();
                }
                if let Some(value) = lower.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let body = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            let response = serde_json::to_vec(&answer).unwrap();
            write!(socket, "HTTP/1.1 {status} Result\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", response.len()).unwrap();
            socket.write_all(&response).unwrap();
            Recorded {
                request: request.trim().into(),
                authorization,
                body,
            }
        });
        (
            Config {
                provider: super::super::Provider::OpenAi,
                endpoint: Some(endpoint),
                model: None,
                api_key: Some("test-openai-key".into()),
            },
            thread,
        )
    }

    fn reply_image() -> (RgbaImage, Value) {
        let image = RgbaImage::from_pixel(3, 2, Rgba([90, 40, 200, 160]));
        let reply = json!({"data":[{"b64_json":BASE64_STANDARD.encode(png(&image))}]});
        (image, reply)
    }

    fn read_image_url(value: &Value) -> RgbaImage {
        let url = value
            .as_str()
            .unwrap()
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        decode(&BASE64_STANDARD.decode(url).unwrap()).unwrap()
    }

    #[test]
    fn connection_check_only_looks_up_the_selected_model() {
        let (mut cfg, server) = mock(200, json!({"id":"custom/model"}));
        cfg.model = Some("custom/model".into());
        assert_eq!(reachable(&cfg).unwrap(), "custom/model");
        let received = server.join().unwrap();
        assert_eq!(received.request, "GET /v1/models/custom%2Fmodel HTTP/1.1");
        assert_eq!(received.authorization, "Bearer test-openai-key");
        assert_eq!(received.body, Value::Null);
    }

    #[test]
    fn generation_authenticates_and_decodes_the_image() {
        let (expected, reply) = reply_image();
        let (cfg, server) = mock(200, reply);
        let actual = request(
            &cfg,
            "a manga runner",
            Some("lettering"),
            None,
            None,
            1200,
            800,
        )
        .unwrap();
        assert_eq!(actual, expected);
        let received = server.join().unwrap();
        assert_eq!(received.request, "POST /v1/images/generations HTTP/1.1");
        assert_eq!(received.authorization, "Bearer test-openai-key");
        assert_eq!(received.body["model"], DEFAULT_MODEL);
        assert_eq!(received.body["size"], "1536x1024");
        assert_eq!(received.body["output_format"], "png");
        assert_eq!(received.body["n"], 1);
        assert!(
            received.body["prompt"]
                .as_str()
                .unwrap()
                .contains("Avoid these elements: lettering")
        );
        assert!(received.body.get("response_format").is_none());
        assert!(received.body.get("negative_prompt").is_none());
        assert!(received.body.get("images").is_none());
    }

    #[test]
    fn masked_edit_transmits_source_and_inverts_white_selection_to_alpha() {
        let (expected, reply) = reply_image();
        let (mut cfg, server) = mock(200, reply);
        cfg.model = Some("gpt-image-1.5".into());
        let source = RgbaImage::from_pixel(3, 1, Rgba([12, 34, 56, 255]));
        let mask = RgbaImage::from_fn(3, 1, |x, _| {
            let value = [0, 128, 255][x as usize];
            Rgba([value, value, value, 255])
        });
        assert_eq!(
            request(
                &cfg,
                "change only the selected area",
                None,
                Some(&source),
                Some(&mask),
                800,
                1200
            )
            .unwrap(),
            expected
        );
        let received = server.join().unwrap();
        assert_eq!(received.request, "POST /v1/images/edits HTTP/1.1");
        assert_eq!(received.body["model"], "gpt-image-1.5");
        assert_eq!(received.body["size"], "1024x1536");
        assert_eq!(
            read_image_url(&received.body["images"][0]["image_url"]),
            source
        );
        let converted = read_image_url(&received.body["mask"]["image_url"]);
        assert_eq!(converted.dimensions(), source.dimensions());
        assert_eq!(converted.get_pixel(0, 0)[3], 255);
        assert_eq!(converted.get_pixel(1, 0)[3], 127);
        assert_eq!(converted.get_pixel(2, 0)[3], 0);
    }

    #[test]
    fn missing_key_and_invalid_edits_fail_before_connecting() {
        let mut cfg = Config {
            provider: super::super::Provider::OpenAi,
            endpoint: Some("http://127.0.0.1:1/v1".into()),
            model: None,
            api_key: None,
        };
        assert!(matches!(reachable(&cfg), Err(GenError::NotConfigured(_))));
        assert!(matches!(
            request(&cfg, "a boat", None, None, None, 800, 600),
            Err(GenError::NotConfigured(_))
        ));
        cfg.api_key = Some("test-openai-key".into());
        let source = RgbaImage::new(3, 2);
        let mask = RgbaImage::new(2, 2);
        assert!(matches!(
            request(&cfg, "edit", None, Some(&source), Some(&mask), 800, 600),
            Err(GenError::Provider(_))
        ));
        assert!(matches!(
            request(&cfg, "edit", None, None, Some(&mask), 800, 600),
            Err(GenError::Provider(_))
        ));
    }

    #[test]
    fn provider_errors_and_missing_images_are_reported() {
        let (cfg, server) = mock(
            401,
            json!({"error":{"message":"invalid test API key","type":"invalid_request_error"}}),
        );
        let error = request(&cfg, "a boat", None, None, None, 800, 600).unwrap_err();
        assert!(matches!(error, GenError::Provider(_)));
        assert!(error.to_string().contains("401"));
        server.join().unwrap();
        let (cfg, server) = mock(200, json!({"data":[]}));
        assert!(matches!(
            request(&cfg, "a boat", None, None, None, 800, 600),
            Err(GenError::Decode(_))
        ));
        server.join().unwrap();
    }
}
