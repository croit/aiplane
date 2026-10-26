// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Branding — the operator-uploaded company logo.
//!
//! Two admin writes (`POST /api/v0/admin/branding`, `DELETE
//! /api/v0/admin/branding/{slot}`) and one public read (`GET /brand/logo`,
//! `GET /brand/logo-dark`) the SPA points its brand lockup and favicon at.
//! Raster uploads are resized down to [`MAX_LOGO_DIMENSION`] before storage;
//! SVG passes through untouched — it is vector data and has no pixels to
//! resample. Every stored image is at most [`MAX_LOGO_BYTES`].

use std::convert::Infallible;
use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode, header};

use image::GenericImageView;
use session_core::i18n::{Lang, t};

use aiplane_core::server::db::branding;
use aiplane_runtime::rama_server::state::RamaState;

use super::{json_error, json_ok, raw_path_segment};

/// The stored size cap for a logo, in bytes.
pub const MAX_LOGO_BYTES: usize = 512 * 1024;
/// The longest side a raster logo is resampled to. Rendered at a few dozen
/// pixels in the UI, this is far beyond what any slot needs.
pub const MAX_LOGO_DIMENSION: u32 = 1200;

/// What went wrong while normalising an upload. The handler translates each
/// variant into the catalogue message the admin sees.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LogoError {
    #[error("the upload is not an image: neither SVG nor a decodable raster format")]
    UnsupportedFormat,
    #[error("the logo is still {bytes} bytes after resizing, over the {max} byte cap")]
    TooLarge { bytes: usize, max: usize },
}

/// Normalise an uploaded logo into its stored form: the bytes to keep and
/// the content type to serve them with.
///
/// SVG is passed through byte-for-byte (it is vector data); raster images
/// are decoded, resampled down to [`MAX_LOGO_DIMENSION`] on the longest side
/// when they exceed it, and re-encoded as PNG so the stored format is one.
pub fn normalize_logo(bytes: &[u8]) -> Result<(Vec<u8>, String), LogoError> {
    if let Some(svg) = as_svg(bytes) {
        if svg.len() > MAX_LOGO_BYTES {
            return Err(LogoError::TooLarge {
                bytes: svg.len(),
                max: MAX_LOGO_BYTES,
            });
        }
        return Ok((svg, "image/svg+xml".to_string()));
    }

    let image = image::load_from_memory(bytes).map_err(|_| LogoError::UnsupportedFormat)?;
    let (width, height) = image.dimensions();
    let scaled = if width.max(height) > MAX_LOGO_DIMENSION {
        image.resize(
            MAX_LOGO_DIMENSION,
            MAX_LOGO_DIMENSION,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        image
    };
    let mut png = Vec::new();
    scaled
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("an in-memory Vec never fails a write");
    if png.len() > MAX_LOGO_BYTES {
        return Err(LogoError::TooLarge {
            bytes: png.len(),
            max: MAX_LOGO_BYTES,
        });
    }
    Ok((png, "image/png".to_string()))
}

/// The bytes as UTF-8 text when they are an SVG document, else `None`.
fn as_svg(bytes: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let trimmed = text.trim_start();
    if trimmed.starts_with('<') && trimmed.to_lowercase().contains("<svg") {
        Some(bytes.to_vec())
    } else {
        None
    }
}

/// `DELETE /api/v0/admin/branding/{slot}` — drop a slot's logo.
pub async fn remove(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (_session, _user) = require_admin_json!(state, req);
    let Some(slot) = raw_path_segment(&req, 0) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            &t(lang, "branding-error-slot"),
        );
    };
    if !branding::is_slot(&slot) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            &t(lang, "branding-error-slot"),
        );
    }
    if let Err(err) = branding::delete(&state.db, &slot).await {
        tracing::warn!(error = %err, "removing the logo");
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &t(lang, "branding-error-store"),
        );
    }
    json_ok(StatusCode::OK, super::Done::OK)
}

/// `GET /api/v0/admin/branding` — which slots carry a logo, so the admin
/// card knows where to show a remove button. The previews themselves come
/// from the public `/brand/logo` routes.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BrandingState {
    pub light: bool,
    pub dark: bool,
}

pub async fn state(State(state): State<Arc<RamaState>>, _req: Request) -> Response {
    let (_session, _user) = require_admin_json!(state, _req);
    let slots = match branding::set_slots(&state.db).await {
        Ok(slots) => slots,
        Err(err) => {
            tracing::warn!(error = %err, "reading the branding state");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "the branding state could not be read — try again",
            );
        }
    };
    json_ok(
        StatusCode::OK,
        BrandingState {
            light: slots.iter().any(|s| s == "light"),
            dark: slots.iter().any(|s| s == "dark"),
        },
    )
}

/// `POST /api/v0/admin/branding` — multipart upload with a `file` part and
/// an optional `slot` part (`light`, the default, or `dark`).
pub async fn upload(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (_session, _user) = require_admin_json!(state, req);

    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !content_type.starts_with("multipart/form-data;") {
        return json_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            &t(lang, "branding-error-multipart"),
        );
    }
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(_msg) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                &t(lang, "branding-error-multipart"),
            );
        }
    };
    let boundary = match multer::parse_boundary(&content_type) {
        Ok(boundary) if !boundary.is_empty() => boundary,
        _ => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                &t(lang, "branding-error-multipart"),
            );
        }
    };
    let stream = rama::futures::stream::once(async move { Ok::<_, Infallible>(bytes) });
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut multipart = multer::Multipart::new(stream, boundary);
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name().map(str::to_string).unwrap_or_default();
                if let Ok(data) = field.bytes().await {
                    parts.push((name, data.to_vec()));
                }
            }
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(error = %err, "reading the logo upload");
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    &t(lang, "branding-error-multipart"),
                );
            }
        }
    }

    let slot = parts
        .iter()
        .find(|(name, _)| name == "slot")
        .map(|(_, value)| String::from_utf8_lossy(value).trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "light".to_string());
    if !branding::is_slot(&slot) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            &t(lang, "branding-error-slot"),
        );
    }
    let Some((_, file)) = parts.iter().find(|(name, _)| name == "file") else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            &t(lang, "branding-error-multipart"),
        );
    };
    if file.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            &t(lang, "branding-error-empty"),
        );
    }

    let (stored, content_type) = match normalize_logo(file) {
        Ok(normalised) => normalised,
        Err(LogoError::UnsupportedFormat) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                &t(lang, "branding-error-format"),
            );
        }
        Err(LogoError::TooLarge { .. }) => {
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "logo_too_large",
                &t(lang, "branding-error-too-large"),
            );
        }
    };

    if let Err(err) = branding::set(&state.db, &slot, stored, &content_type).await {
        tracing::warn!(error = %err, "storing the logo");
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &t(lang, "branding-error-store"),
        );
    }
    json_ok(StatusCode::CREATED, super::Done::OK)
}

/// `GET /brand/logo` and `GET /brand/logo-dark` — the public read the SPA's
/// brand lockup and favicon point at. No session: the logo is not a secret,
/// and the lockup renders on the login screen. A missing slot is a plain
/// 404; the SPA falls back to the built-in mark on `onerror`.
pub async fn serve(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let slot = if req.uri().path().ends_with("-dark") {
        "dark"
    } else {
        "light"
    };
    match branding::get(&state.db, slot).await {
        Ok(Some(logo)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, logo.content_type)
            // The logo changes the moment an admin re-uploads it; the SPA
            // must see the new bytes on its next load.
            .header(header::CACHE_CONTROL, "no-cache")
            .body(logo.bytes.into())
            .expect("static logo response"),
        Ok(None) => json_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no logo is set for this theme",
        ),
        Err(err) => {
            tracing::warn!(error = %err, "serving the logo");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "the logo could not be read — try again",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster_png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x % 251) as u8, (y % 251) as u8, 128, 255])
        });
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    #[test]
    fn normalize_passes_svg_through_untouched() {
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>";
        let (bytes, content_type) = normalize_logo(svg).unwrap();
        assert_eq!(bytes, svg);
        assert_eq!(content_type, "image/svg+xml");
    }

    #[test]
    fn normalize_resizes_an_oversized_raster_down() {
        let png = raster_png(2000, 1000);
        let (bytes, content_type) = normalize_logo(&png).unwrap();
        assert_eq!(content_type, "image/png");
        let resized = image::load_from_memory(&bytes).unwrap();
        let (width, height) = resized.dimensions();
        assert!(width <= MAX_LOGO_DIMENSION);
        assert!(height <= MAX_LOGO_DIMENSION);
        assert!(width > height, "the aspect ratio is kept");
    }

    #[test]
    fn normalize_keeps_a_small_raster_at_its_size() {
        let png = raster_png(100, 50);
        let (bytes, content_type) = normalize_logo(&png).unwrap();
        assert_eq!(content_type, "image/png");
        let kept = image::load_from_memory(&bytes).unwrap();
        assert_eq!(kept.dimensions(), (100, 50));
    }

    #[test]
    fn normalize_rejects_an_oversized_svg() {
        let mut svg = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
        svg.push_str(&"<path d=\"M0 0\"/>".repeat((MAX_LOGO_BYTES / 12) + 10));
        svg.push_str("</svg>");
        let err = normalize_logo(svg.as_bytes()).unwrap_err();
        assert!(matches!(err, LogoError::TooLarge { .. }), "got {err:?}");
    }

    #[test]
    fn normalize_rejects_bytes_that_are_no_image() {
        assert_eq!(
            normalize_logo(b"definitely not an image").unwrap_err(),
            LogoError::UnsupportedFormat
        );
        assert_eq!(
            normalize_logo(b"").unwrap_err(),
            LogoError::UnsupportedFormat
        );
    }

    #[test]
    fn the_slot_gate_covers_only_light_and_dark() {
        assert!(branding::is_slot("light"));
        assert!(branding::is_slot("dark"));
        assert!(!branding::is_slot("LIGHT"));
        assert!(!branding::is_slot(""));
    }
}
