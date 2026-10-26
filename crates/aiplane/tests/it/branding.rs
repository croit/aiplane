// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Branding — the operator-uploaded company logo. The admin write pair
//! (`POST /api/v0/admin/branding`, `DELETE /api/v0/admin/branding/{slot}`),
//! the public reads (`GET /brand/logo`, `GET /brand/logo-dark`) and the
//! normalisation contract: raster uploads come back resized PNGs, SVG
//! passes through, everything over the cap is refused.

use crate::common;

use common::Service as _;
use image::GenericImageView;
use rama::http::body::util::BodyExt;
use rama::http::{Body, Method, Request, StatusCode, header};

async fn setup() -> (
    std::sync::Arc<aiplane::rama_server::RamaState>,
    String,
    String,
) {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let state = std::sync::Arc::new(state);
    let admin = common::seed_session(&state, "boss", "boss@example.com").await;
    let pleb = common::seed_session(&state, "pleb", "pleb@example.com").await;
    aiplane_core::server::db::users::upsert(
        &state.db,
        &aiplane_core::server::db::users::User {
            id: "boss".into(),
            email: "boss@example.com".into(),
            name: None,
            roles: vec!["admin".into()],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    aiplane_core::server::db::gateway_groups::upsert_group(&state.db, "admin", "", true, false)
        .await
        .unwrap();
    aiplane_core::server::db::gateway_groups::set_mappings_for_group(
        &state.db,
        "admin",
        &["admin".to_string()],
    )
    .await
    .unwrap();
    (state, admin, pleb)
}

fn multipart(file_name: &str, file_bytes: &[u8], slot: Option<&str>) -> (String, Vec<u8>) {
    let boundary = "branding-test-boundary";
    let mut body: Vec<u8> = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(b"\r\n");
    if let Some(slot) = slot {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"slot\"\r\n\r\n{slot}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn upload_req(cookie: &str, file_name: &str, file_bytes: &[u8], slot: Option<&str>) -> Request {
    let (content_type, body) = multipart(file_name, file_bytes, slot);
    Request::builder()
        .method(Method::POST)
        .uri("/api/v0/admin/branding")
        .header("cookie", format!("id={cookie}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap()
}

fn json_req(method: Method, uri: &str, cookie: Option<&str>) -> Request {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", format!("id={cookie}"));
    }
    builder.body(Body::empty()).unwrap()
}

fn test_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([(x % 251) as u8, (y % 251) as u8, 128, 255])
    });
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    png
}

async fn body_bytes(resp: rama::http::Response) -> rama::bytes::Bytes {
    resp.into_body().collect().await.unwrap().to_bytes()
}

#[tokio::test]
async fn brand_routes_answer_404_while_no_logo_is_set() {
    let (state, _admin, _pleb) = setup().await;
    let app = common::app((*state).clone());
    for uri in ["/brand/logo", "/brand/logo-dark"] {
        let resp = app.serve(json_req(Method::GET, uri, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn logo_upload_requires_the_admin_role() {
    let (state, _admin, pleb) = setup().await;
    let app = common::app((*state).clone());
    let png = test_png(200, 100);

    let anonymous = app
        .serve(upload_req("", "logo.png", &png, None))
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let non_admin = app
        .serve(upload_req(&pleb, "logo.png", &png, None))
        .await
        .unwrap();
    assert_eq!(non_admin.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn logo_upload_stores_and_serves_a_resized_png() {
    let (state, admin, _pleb) = setup().await;
    let app = common::app((*state).clone());
    let big = test_png(2000, 1000);

    let created = app
        .serve(upload_req(&admin, "logo.png", &big, None))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);

    let state_now = app
        .serve(json_req(
            Method::GET,
            "/api/v0/admin/branding",
            Some(&admin),
        ))
        .await
        .unwrap();
    assert_eq!(state_now.status(), StatusCode::OK);
    assert!(
        String::from_utf8_lossy(&body_bytes(state_now).await).contains("\"light\":true"),
        "the light slot is set, the dark one is not"
    );

    let served = app
        .serve(json_req(Method::GET, "/brand/logo", None))
        .await
        .unwrap();
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(
        served.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    let bytes = body_bytes(served).await;
    let decoded = image::load_from_memory(&bytes).unwrap();
    let (width, height) = decoded.dimensions();
    assert!(width <= 1200, "resized down, was 2000 wide");
    assert!(height <= 1200);
    assert!(width > height, "the aspect ratio is kept");
}

#[tokio::test]
async fn logo_upload_passes_an_svg_through() {
    let (state, admin, _pleb) = setup().await;
    let app = common::app((*state).clone());
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>";

    let created = app
        .serve(upload_req(&admin, "logo.svg", svg, Some("dark")))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);

    let served = app
        .serve(json_req(Method::GET, "/brand/logo-dark", None))
        .await
        .unwrap();
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(
        served.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/svg+xml"
    );
    assert_eq!(&body_bytes(served).await[..], svg);

    let light_still_empty = app
        .serve(json_req(Method::GET, "/brand/logo", None))
        .await
        .unwrap();
    assert_eq!(light_still_empty.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn logo_upload_refuses_an_oversized_svg() {
    let (state, admin, _pleb) = setup().await;
    let app = common::app((*state).clone());
    let mut svg = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    svg.push_str(&"<path d=\"M0 0\"/>".repeat(60_000));
    svg.push_str("</svg>");
    assert!(svg.len() > 512 * 1024);

    let resp = app
        .serve(upload_req(&admin, "logo.svg", svg.as_bytes(), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let bytes = body_bytes(resp).await;
    let body = String::from_utf8_lossy(&bytes);
    assert!(body.contains("logo_too_large"), "{body}");

    let still_empty = app
        .serve(json_req(Method::GET, "/brand/logo", None))
        .await
        .unwrap();
    assert_eq!(still_empty.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn logo_upload_refuses_bytes_that_are_no_image() {
    let (state, admin, _pleb) = setup().await;
    let app = common::app((*state).clone());

    let resp = app
        .serve(upload_req(
            &admin,
            "notes.txt",
            b"not an image at all",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn logo_remove_drops_the_slot() {
    let (state, admin, _pleb) = setup().await;
    let app = common::app((*state).clone());
    let png = test_png(200, 100);
    assert_eq!(
        app.serve(upload_req(&admin, "logo.png", &png, None))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );

    let removed = app
        .serve(json_req(
            Method::DELETE,
            "/api/v0/admin/branding/light",
            Some(&admin),
        ))
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::OK);

    let gone = app
        .serve(json_req(Method::GET, "/brand/logo", None))
        .await
        .unwrap();
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn logo_remove_refuses_an_unknown_slot() {
    let (state, admin, _pleb) = setup().await;
    let app = common::app((*state).clone());
    let resp = app
        .serve(json_req(
            Method::DELETE,
            "/api/v0/admin/branding/bogus",
            Some(&admin),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn logo_upload_refuses_a_missing_file_part() {
    let (state, admin, _pleb) = setup().await;
    let app = common::app((*state).clone());
    // Only a `slot` part, no `file` part at all.
    let boundary = "branding-test-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"slot\"\r\n\r\nlight\r\n--{boundary}--\r\n"
    );
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v0/admin/branding")
        .header("cookie", format!("id={admin}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = app.serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
