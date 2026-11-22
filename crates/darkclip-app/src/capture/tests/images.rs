use super::*;

fn png_copy(name: &str, bytes: Vec<u8>) -> FakeCopy {
    FakeCopy {
        formats: vec![(pf(name, Some(Mime::ImagePng), bytes.len()), bytes)],
        markers: Vec::new(),
        source: None,
    }
}

/// The same 3x2 picture, encoded two ways (different filter and compression).
fn two_encodings() -> (Vec<u8>, Vec<u8>) {
    let px: Vec<u8> = (0..24u8).map(|i| i.wrapping_mul(9)).collect();
    let enc = |filter: png::Filter, comp: png::Compression| {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 3, 2);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.set_filter(filter);
        e.set_compression(comp);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&px).unwrap();
        w.finish().unwrap();
        out
    };
    (
        enc(png::Filter::NoFilter, png::Compression::Fast),
        enc(png::Filter::Sub, png::Compression::High),
    )
}

#[test]
fn same_pixels_different_encoding_dedupes() {
    let (a, b) = two_encodings();
    assert_ne!(a, b);
    let h = start(PrivacyPolicy::default());
    h.fake.copy(png_copy("PNG", a));
    let (first, _) = h.wait_added();
    h.fake.copy(png_copy("PNG", b));
    h.wait_for("ClipBumped", |e| *e == AppEvent::ClipBumped(first));
    assert_eq!(h.clip_count(), 1);
}

#[test]
fn corrupt_png_still_stored_with_fallback_hash() {
    let h = start(PrivacyPolicy::default());
    let bytes = b"\x89PNG\r\n\x1a\nnot really a png".to_vec();
    h.fake.copy(png_copy("PNG", bytes.clone()));
    let (id, _) = h.wait_added();
    let ro = h.ro();
    let clip = ro.get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.kind, ClipType::Image);
    assert!(clip.metadata.image_thumb_sha256.is_none());
    let want = canonical_hash(
        ClipType::Image,
        &[RawRepresentation {
            mime: Mime::ImagePng,
            platform_format: "PNG".into(),
            bytes,
            width: None,
            height: None,
            truncated: false,
        }],
        None,
    );
    assert_eq!(clip.content_hash, want);
}

#[test]
fn masked_image_clip_has_no_thumbnail() {
    let (a, _) = two_encodings();
    let html = format!("<p>{}</p>", api_key_text()).into_bytes();
    let h = start(masked_policy());
    h.fake.copy(FakeCopy {
        formats: vec![
            (pf("PNG", Some(Mime::ImagePng), a.len()), a),
            (pf("HTML Format", Some(Mime::TextHtml), html.len()), html),
        ],
        markers: Vec::new(),
        source: None,
    });
    let (id, _) = h.wait_added();
    let clip = h.ro().get_clip(&id).unwrap().unwrap();
    assert_eq!(clip.kind, ClipType::Image);
    assert_eq!(clip.sensitivity.level, SensitivityLevel::Masked);
    assert!(clip.metadata.image_thumb_sha256.is_none());
}

#[test]
fn image_clip_has_thumbnail_and_dimensions() {
    let px: Vec<u8> = vec![120; 600 * 40 * 4];
    let mut bytes = Vec::new();
    let mut e = png::Encoder::new(&mut bytes, 600, 40);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let mut w = e.write_header().unwrap();
    w.write_image_data(&px).unwrap();
    w.finish().unwrap();
    let h = start(PrivacyPolicy::default());
    h.fake.copy(png_copy("PNG", bytes));
    let (id, _) = h.wait_added();
    let ro = h.ro();
    let clip = ro.get_clip(&id).unwrap().unwrap();
    assert!(clip.metadata.image_thumb_sha256.is_some());
    let rep = &clip.representations[0];
    assert_eq!((rep.width, rep.height), (Some(600), Some(40)));
    assert_eq!(clip.preview, "Image 600\u{00d7}40");
}
