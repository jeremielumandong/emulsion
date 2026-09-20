//! Generative fill and text-to-image through local SD, OpenAI, or Google.
//! Cloud requests send prompts and fill context to the selected provider.
//! Results land as new, labelled layers with provenance.

mod google;
mod openai;

use crate::jobs::Job;
use base64::prelude::{BASE64_STANDARD, Engine};
use emulsion_raster::{IRect, Mask, Raster, select};
use image::{ImageFormat, RgbaImage, imageops::FilterType};
use std::io::Cursor;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// Automatic1111 / Forge `sdapi/v1` on this machine or the LAN.
    A1111,
    OpenAi,
    Google,
}

impl Provider {
    pub fn parse(id: &str) -> Option<Provider> {
        Some(match id.trim().to_lowercase().as_str() {
            "a1111" | "automatic1111" | "forge" | "sd-webui" | "local" => Provider::A1111,
            "openai" => Provider::OpenAi,
            "google" | "gemini" => Provider::Google,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Provider::A1111 => "a1111",
            Provider::OpenAi => "openai",
            Provider::Google => "google",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::A1111 => "Local SD server (A1111 / Forge)",
            Provider::OpenAi => "OpenAI Images",
            Provider::Google => "Google Gemini Images",
        }
    }

    pub fn default_endpoint(self) -> &'static str {
        match self {
            Provider::A1111 => "http://127.0.0.1:7860",
            Provider::OpenAi => "https://api.openai.com/v1",
            Provider::Google => "https://generativelanguage.googleapis.com/v1beta",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::A1111 => "",
            Self::OpenAi => openai::DEFAULT_MODEL,
            Self::Google => google::DEFAULT_MODEL,
        }
    }
}

pub const PROVIDERS: [Provider; 3] = [Provider::A1111, Provider::OpenAi, Provider::Google];

/// The person's server choice, from Settings.
#[derive(Clone)]
pub struct Config {
    pub provider: Provider,
    /// Base URL of the server; the provider's default when empty.
    pub endpoint: Option<String>,
    /// Checkpoint name to ask the server to use, if any.
    pub model: Option<String>,
    pub api_key: Option<String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("provider", &self.provider)
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), GenError> {
        if self.provider != Provider::A1111
            && self
                .api_key
                .as_deref()
                .is_none_or(|key| key.trim().is_empty())
        {
            return Err(GenError::NotConfigured(format!(
                "add your {} API key in Settings › Image generation",
                self.provider.label()
            )));
        }
        Ok(())
    }

    fn endpoint(&self) -> String {
        self.endpoint
            .as_deref()
            .filter(|e| !e.trim().is_empty())
            .unwrap_or(self.provider.default_endpoint())
            .trim()
            .trim_end_matches('/')
            .to_string()
    }

    /// The provenance tag written on generated layers.
    pub fn model_id(&self) -> String {
        match self.model.as_deref().filter(|m| !m.trim().is_empty()) {
            Some(m) => format!("{}/{m}", self.provider.id()),
            None if self.provider != Provider::A1111 => {
                format!("{}/{}", self.provider.id(), self.provider.default_model())
            }
            None => self.provider.id().to_string(),
        }
    }
}

#[derive(Debug)]
pub enum GenError {
    NotConfigured(String),
    Http(String),
    Provider(String),
    Decode(String),
    Cancelled,
}

impl std::fmt::Display for GenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenError::NotConfigured(s) => write!(f, "image generation is not set up: {s}"),
            GenError::Http(s) => write!(f, "could not reach the image server: {s}"),
            GenError::Provider(s) => write!(f, "the image server said: {s}"),
            GenError::Decode(s) => write!(f, "could not read the generated image: {s}"),
            GenError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for GenError {}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_body(Some(Duration::from_secs(600)))
            .timeout_recv_response(Some(Duration::from_secs(600)))
            .http_status_as_error(false)
            .build(),
    )
}

fn png(img: &RgbaImage) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, ImageFormat::Png);
    out.into_inner()
}

fn decode(bytes: &[u8]) -> Result<RgbaImage, GenError> {
    image::load_from_memory(bytes)
        .map(|i| i.to_rgba8())
        .map_err(|e| GenError::Decode(e.to_string()))
}

/// The response body, or the server's error message for a bad status.
fn check(resp: &mut ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, GenError> {
    let status = resp.status().as_u16();
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| GenError::Http(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(GenError::Provider(provider_error(status, &bytes)));
    }
    Ok(bytes)
}

/// Keep provider diagnostics useful without embedding pages of quota metadata in the UI.
fn provider_error(status: u16, bytes: &[u8]) -> String {
    let value = serde_json::from_slice::<serde_json::Value>(bytes).ok();
    let raw = value.as_ref().and_then(|v| {
        v.get("detail")
            .or_else(|| v.pointer("/error/message"))
            .or_else(|| v.get("error"))
            .or_else(|| v.get("message"))
            .and_then(serde_json::Value::as_str)
    });
    let fallback = String::from_utf8_lossy(bytes);
    let message = raw.unwrap_or(&fallback);
    let zero_free_quota = message.lines().any(|line| {
        line.contains("free_tier")
            && line.split_once("limit:").is_some_and(|(_, limit)| {
                limit.trim_start().split([',', ' ', '\n']).next() == Some("0")
            })
    }) || value
        .as_ref()
        .and_then(|v| v.pointer("/error/details"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|details| {
            details
                .iter()
                .filter_map(|d| d.get("violations").and_then(serde_json::Value::as_array))
                .flatten()
                .any(|v| {
                    let metric = v
                        .get("quotaMetric")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    let zero = v
                        .get("quotaValue")
                        .is_some_and(|n| n.as_u64() == Some(0) || n.as_str() == Some("0"));
                    metric.contains("generativelanguage.googleapis.com/")
                        && metric.contains("free_tier")
                        && zero
                })
        });
    if status == 429 && zero_free_quota {
        return "HTTP 429: Google image quota is 0 for this API project. Check billing, credits, and model quotas in Google AI Studio; confirm your API key belongs to that project. Waiting alone will not fix a zero quota.".into();
    }
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut summary: String = normalized.chars().take(320).collect();
    if normalized.chars().count() > 320 {
        summary.push('…');
    }
    format!("HTTP {status}: {summary}")
}

/// Is a server answering at the configured address?
pub fn reachable(cfg: &Config) -> Result<String, GenError> {
    cfg.validate()?;
    match cfg.provider {
        Provider::OpenAi => return openai::reachable(cfg),
        Provider::Google => return google::reachable(cfg),
        Provider::A1111 => {}
    }
    let mut resp = agent()
        .get(format!("{}/sdapi/v1/options", cfg.endpoint()))
        .call()
        .map_err(|e| GenError::Http(e.to_string()))?;
    if resp.status().as_u16() == 404 {
        return Err(GenError::Provider(
            "HTTP 404: the Stable Diffusion API was not found. Restart A1111 / Forge with --api and use the base address, e.g. http://127.0.0.1:7860".into(),
        ));
    }
    let bytes = check(&mut resp)?;
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| GenError::Decode(e.to_string()))?;
    Ok(v.get("sd_model_checkpoint")
        .and_then(|m| m.as_str())
        .unwrap_or("a Stable Diffusion server")
        .to_string())
}

/// A size the server accepts: multiples of 64, longest side ≤ 1024.
fn fit(w: u32, h: u32) -> (u32, u32) {
    let s = (1024.0 / w.max(h) as f32).min(1.0);
    let r = |v: u32| (((v as f32 * s) / 64.0).round().max(1.0) as u32 * 64).clamp(64, 1024);
    (r(w), r(h))
}

/// Ask the server for `w`×`h` pixels from `prompt`, optionally repainting
/// `image` where `mask` is white.
fn request(
    cfg: &Config,
    prompt: &str,
    negative: Option<&str>,
    image: Option<&RgbaImage>,
    mask: Option<&RgbaImage>,
    w: u32,
    h: u32,
) -> Result<RgbaImage, GenError> {
    cfg.validate()?;
    let base = cfg.endpoint();
    match cfg.provider {
        Provider::OpenAi => openai::request(cfg, prompt, negative, image, mask, w, h),
        Provider::Google => google::request(cfg, prompt, negative, image, mask, w, h),
        Provider::A1111 => {
            let mut body = serde_json::json!({
                "prompt": prompt,
                "negative_prompt": negative.unwrap_or(""),
                "width": w, "height": h, "steps": 28, "cfg_scale": 7.0,
            });
            if let Some(m) = cfg.model.as_deref().filter(|m| !m.trim().is_empty()) {
                body["override_settings"] = serde_json::json!({ "sd_model_checkpoint": m });
            }
            let url = if let (Some(img), Some(mask)) = (image, mask) {
                body["init_images"] = serde_json::json!([BASE64_STANDARD.encode(png(img))]);
                body["mask"] = serde_json::json!(BASE64_STANDARD.encode(png(mask)));
                body["denoising_strength"] = serde_json::json!(1.0);
                body["inpainting_fill"] = serde_json::json!(1);
                body["inpaint_full_res"] = serde_json::json!(false);
                body["mask_blur"] = serde_json::json!(4);
                format!("{base}/sdapi/v1/img2img")
            } else {
                format!("{base}/sdapi/v1/txt2img")
            };
            let mut resp = agent()
                .post(url)
                .send_json(body)
                .map_err(|e| GenError::Http(e.to_string()))?;
            let bytes = check(&mut resp)?;
            let v: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| GenError::Decode(e.to_string()))?;
            let b64 = v
                .pointer("/images/0")
                .and_then(|s| s.as_str())
                .ok_or_else(|| GenError::Decode("no image in the reply".into()))?;
            let b64 = b64.split_once(',').map(|(_, d)| d).unwrap_or(b64);
            let raw = BASE64_STANDARD
                .decode(b64)
                .map_err(|e| GenError::Decode(e.to_string()))?;
            decode(&raw)
        }
    }
}

fn to_image(r: &Raster) -> RgbaImage {
    RgbaImage::from_raw(r.width(), r.height(), r.to_srgba8()).expect("raster size")
}

/// Fill the white part of `hole` on `image` from `prompt`. Returns the
/// new layer (only the filled area is opaque) and where it sits.
pub fn fill(
    cfg: &Config,
    image: &Raster,
    hole: &Mask,
    prompt: &str,
    negative: Option<&str>,
    job: &Job,
) -> Result<(Raster, IRect), GenError> {
    let b = select::bounds(hole);
    if b.is_empty() {
        return Err(GenError::Provider("nothing selected to fill".into()));
    }
    // Context around the hole helps the model match its surroundings.
    let margin = (b.w.max(b.h) / 3).max(32);
    let crop = IRect::new(
        b.x - margin,
        b.y - margin,
        b.w + 2 * margin,
        b.h + 2 * margin,
    )
    .intersect(&image.bounds());
    let (cw, ch) = (crop.w as u32, crop.h as u32);
    let src = {
        let px: Vec<[u16; 4]> = image.read_rect(crop);
        to_image(&Raster::from_pixels(cw, ch, [0; 4], &px))
    };
    let mask_px: Vec<u8> = hole.read_rect(crop);
    let mask_img = RgbaImage::from_raw(
        cw,
        ch,
        mask_px.iter().flat_map(|v| [*v, *v, *v, 255]).collect(),
    )
    .expect("mask size");
    let (rw, rh) = fit(cw, ch);
    let src_r = image::imageops::resize(&src, rw, rh, FilterType::Lanczos3);
    let mask_r = image::imageops::resize(&mask_img, rw, rh, FilterType::Triangle);
    job.progress(0.1);
    if job.cancelled() {
        return Err(GenError::Cancelled);
    }
    let out = request(cfg, prompt, negative, Some(&src_r), Some(&mask_r), rw, rh)?;
    if job.cancelled() {
        return Err(GenError::Cancelled);
    }
    job.progress(0.9);
    let out = if out.width() != cw || out.height() != ch {
        image::imageops::resize(&out, cw, ch, FilterType::Lanczos3)
    } else {
        out
    };
    // Keep the generated pixels only where the hole is, with a soft edge.
    let soft = select::feather(&Mask::from_pixels(cw, ch, 0, &mask_px), 2.0);
    let cover: Vec<u8> = soft.read_rect(soft.bounds());
    let mut rgba = out.into_raw();
    for (i, c) in cover.iter().enumerate() {
        let coverage = (*c).min(mask_px[i]);
        rgba[i * 4 + 3] = ((rgba[i * 4 + 3] as u32 * coverage as u32) / 255) as u8;
    }
    Ok((Raster::from_srgba8(cw, ch, &rgba), crop))
}

/// A whole new picture of `w`×`h` from `prompt`.
pub fn text_to_image(
    cfg: &Config,
    prompt: &str,
    negative: Option<&str>,
    w: u32,
    h: u32,
    job: &Job,
) -> Result<Raster, GenError> {
    if job.cancelled() {
        return Err(GenError::Cancelled);
    }
    let (rw, rh) = fit(w, h);
    job.progress(0.1);
    let out = request(cfg, prompt, negative, None, None, rw, rh)?;
    if job.cancelled() {
        return Err(GenError::Cancelled);
    }
    job.progress(0.9);
    let out = if out.width() != w || out.height() != h {
        image::imageops::resize(&out, w, h, FilterType::Lanczos3)
    } else {
        out
    };
    Ok(Raster::from_srgba8(w, h, &out.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_free_quota_gives_project_setup_instead_of_retry_advice() {
        let payload = serde_json::json!({"error": {"message": "You exceeded your current quota.\n* Quota exceeded for metric: generativelanguage.googleapis.com/generate_content_free_tier_requests, limit: 0, model: gemini-3.1-flash-image\nPlease retry in 28.09s."}});
        let message = provider_error(429, &serde_json::to_vec(&payload).unwrap());
        assert!(message.contains("Google image quota is 0"));
        assert!(message.contains("billing, credits"));
        assert!(!message.contains("retry in"));
        assert!(!message.contains('\n'));

        let structured = serde_json::json!({"error": {"message": "RESOURCE_EXHAUSTED", "details": [{"violations": [{"quotaMetric": "generativelanguage.googleapis.com/generate_content_free_tier_requests", "quotaValue": "0"}]}]}});
        assert_eq!(
            message,
            provider_error(429, &serde_json::to_vec(&structured).unwrap())
        );
        let limited = serde_json::json!({"error": {"message": "Quota exceeded for metric: generate_content_free_tier_requests, limit: 10. Please retry in 28s."}});
        let temporary = provider_error(429, &serde_json::to_vec(&limited).unwrap());
        assert!(temporary.contains("retry in 28s"));
        assert!(!temporary.contains("quota is 0"));
    }

    #[test]
    fn provider_errors_are_bounded_and_flattened() {
        let payload = serde_json::json!({"error": {"message": "long error\n".repeat(1000)}});
        let message = provider_error(500, &serde_json::to_vec(&payload).unwrap());
        assert!(message.chars().count() <= 331);
        assert!(!message.contains('\n'));
        assert!(message.ends_with('…'));
    }

    #[test]
    fn cloud_config_requires_key_and_redacts_it() {
        for provider in [Provider::OpenAi, Provider::Google] {
            let mut cfg = Config {
                provider,
                endpoint: None,
                model: None,
                api_key: None,
            };
            assert!(matches!(cfg.validate(), Err(GenError::NotConfigured(_))));
            cfg.api_key = Some("private-test-key".into());
            assert!(cfg.validate().is_ok());
            assert!(!format!("{cfg:?}").contains("private-test-key"));
            assert_eq!(
                cfg.model_id(),
                format!("{}/{}", provider.id(), provider.default_model())
            );
        }
    }

    #[test]
    fn cancelled_generation_does_not_contact_provider() {
        let job = Job::new();
        job.cancel();
        let cfg = Config {
            provider: Provider::OpenAi,
            endpoint: None,
            model: None,
            api_key: None,
        };
        assert!(matches!(
            text_to_image(&cfg, "test", None, 64, 64, &job),
            Err(GenError::Cancelled)
        ));
    }

    #[test]
    fn sizes_fit_and_config_resolves() {
        assert_eq!(fit(3000, 2000), (1024, 704));
        assert_eq!(fit(500, 500), (512, 512));
        assert_eq!(fit(100, 2000), (64, 1024));
        assert_eq!(Provider::parse("Forge"), Some(Provider::A1111));
        let cfg = Config {
            provider: Provider::A1111,
            endpoint: Some("http://box.local:7860/".into()),
            model: Some("juggernaut".into()),
            api_key: None,
        };
        assert_eq!(cfg.endpoint(), "http://box.local:7860");
        assert_eq!(cfg.model_id(), "a1111/juggernaut");
        // No server here: a clean transport error, not a panic.
        let cfg = Config {
            provider: Provider::A1111,
            endpoint: Some("http://127.0.0.1:1".into()),
            model: None,
            api_key: None,
        };
        assert!(matches!(reachable(&cfg), Err(GenError::Http(_))));
    }
}
