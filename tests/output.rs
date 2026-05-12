use std::fs;
use std::time::{Duration, SystemTime};

use codex_image::diagnostics::CliError;
use codex_image::output::{
    remove_batch_manifest_if_exists, write_batch_generation_manifest,
    write_generation_output_from_files, write_generation_output_from_sources, GeneratedImageSource,
};
use tempfile::{tempdir, NamedTempFile};

#[test]
fn output_writes_single_image_and_manifest_with_expected_contract() {
    let temp = tempdir().expect("tempdir should create");
    let source_dir = temp.path().join("source");
    fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("generated.png");
    fs::write(&source, b"png-bytes").unwrap();
    let out_dir = temp.path().join("images");

    let manifest =
        write_generation_output_from_files("sunrise", "gpt-image-2", &out_dir, &[source])
            .expect("output write should succeed");

    let image_path = out_dir.join("image-0001.png");
    let manifest_path = out_dir.join("manifest.json");

    assert!(image_path.exists(), "image file should exist");
    assert!(manifest_path.exists(), "manifest file should exist");
    assert_eq!(fs::read(&image_path).unwrap(), b"png-bytes");

    assert_eq!(manifest.prompt, "sunrise");
    assert_eq!(manifest.model, "gpt-image-2");
    assert_eq!(manifest.images.len(), 1);
    assert_eq!(manifest.images[0].index, 1);
    assert_eq!(manifest.images[0].format, "png");
    assert_eq!(manifest.images[0].byte_count, 9);
    assert_eq!(manifest.images[0].path, image_path.to_string_lossy());
    assert_eq!(manifest.manifest_path, manifest_path.to_string_lossy());

    let manifest_text = fs::read_to_string(&manifest_path).unwrap();
    let manifest_json: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest_json["prompt"], "sunrise");
    assert_eq!(manifest_json["model"], "gpt-image-2");
    assert_eq!(
        manifest_json["images"][0]["path"],
        image_path.to_string_lossy().to_string()
    );
}

#[test]
fn output_writes_multiple_images_with_deterministic_filenames() {
    let temp = tempdir().expect("tempdir should create");
    let source_dir = temp.path().join("source");
    fs::create_dir_all(&source_dir).unwrap();
    let sources = [
        source_dir.join("first.png"),
        source_dir.join("second.webp"),
        source_dir.join("third.jpeg"),
    ];
    fs::write(&sources[0], b"first").unwrap();
    fs::write(&sources[1], b"second").unwrap();
    fs::write(&sources[2], b"third").unwrap();
    let out_dir = temp.path().join("images");

    let manifest = write_generation_output_from_files("multi", "gpt-image-2", &out_dir, &sources)
        .expect("output write should succeed");

    let expected = [
        out_dir.join("image-0001.png"),
        out_dir.join("image-0002.webp"),
        out_dir.join("image-0003.jpeg"),
    ];

    for (idx, path) in expected.iter().enumerate() {
        assert!(path.exists(), "{} should exist", path.display());
        assert_eq!(manifest.images[idx].path, path.to_string_lossy());
        assert_eq!(manifest.images[idx].index, idx + 1);
    }
}

#[test]
fn output_manifest_redacts_source_path_and_token_sentinels() {
    let temp = tempdir().expect("tempdir should create");
    let source_dir = temp.path().join("source-access-token-Bearer");
    fs::create_dir_all(&source_dir).unwrap();
    let source = source_dir.join("generated-b64_json.png");
    fs::write(
        &source,
        b"binary access-token refresh-token id-token Bearer b64_json",
    )
    .unwrap();
    let out_dir = temp.path().join("images");

    let manifest =
        write_generation_output_from_files("safe prompt", "gpt-image-2", &out_dir, &[source])
            .expect("output write should succeed");

    let manifest_text = fs::read_to_string(out_dir.join("manifest.json")).unwrap();
    let json_text = serde_json::to_string(&manifest).unwrap();

    for forbidden in [
        "b64_json",
        "access-token",
        "refresh-token",
        "id-token",
        "Bearer",
    ] {
        assert!(
            !manifest_text.contains(forbidden),
            "manifest should not contain {forbidden}"
        );
        assert!(
            !json_text.contains(forbidden),
            "serialized contract should not contain {forbidden}"
        );
    }
}

#[test]
fn output_rejects_trusted_source_modified_before_not_before_without_copying() {
    let temp = tempdir().expect("tempdir should create");
    let source = temp.path().join("stale-source.png");
    fs::write(&source, b"stale-bytes").unwrap();
    let out_dir = temp.path().join("images");
    let trusted_source =
        GeneratedImageSource::trusted_after(source, SystemTime::now() + Duration::from_secs(60));

    let err = write_generation_output_from_sources(
        "prompt secret",
        "gpt-image-2",
        &out_dir,
        &[trusted_source],
    )
    .expect_err("trusted source older than not_before must fail");

    assert!(matches!(
        err,
        CliError::ImageGenerationResponseContract { .. }
    ));
    assert_eq!(
        err.error_envelope().error.code,
        "response_contract.image_generation"
    );
    assert!(
        !out_dir.join("manifest.json").exists(),
        "failed freshness validation must not write manifest"
    );
    assert!(
        !out_dir.join("image-0001.png").exists(),
        "failed freshness validation must not copy image"
    );
}

#[test]
fn output_empty_image_list_maps_to_response_contract_error() {
    let temp = tempdir().expect("tempdir should create");
    let out_dir = temp.path().join("images");

    let err = write_generation_output_from_files("empty", "gpt-image-2", &out_dir, &[])
        .expect_err("empty image list must fail");

    assert!(matches!(
        err,
        CliError::ImageGenerationResponseContract { .. }
    ));
}

#[test]
fn output_missing_source_maps_to_response_contract_error() {
    let temp = tempdir().expect("tempdir should create");
    let out_dir = temp.path().join("images");
    let missing = temp.path().join("missing.png");

    let err = write_generation_output_from_files("missing", "gpt-image-2", &out_dir, &[missing])
        .expect_err("missing source must fail");

    assert!(matches!(
        err,
        CliError::ImageGenerationResponseContract { .. }
    ));
}

#[test]
fn output_existing_file_target_maps_to_filesystem_error() {
    let temp = tempdir().expect("tempdir should create");
    let source = temp.path().join("source.png");
    fs::write(&source, b"bytes").unwrap();
    let file = NamedTempFile::new().expect("file should create");
    let out_path = file.path();

    let err = write_generation_output_from_files("prompt", "gpt-image-2", out_path, &[source])
        .expect_err("existing file path should fail");

    assert!(matches!(
        err,
        CliError::OutputWriteFailed | CliError::OutputVerificationFailed
    ));
    assert_eq!(
        err.error_envelope().error.code,
        "filesystem.output_write_failed"
    );
}

#[test]
fn batch_output_writes_root_manifest_with_deterministic_item_order() {
    let temp = tempdir().expect("tempdir should create");
    let source_dir = temp.path().join("source-access-token-Bearer");
    fs::create_dir_all(&source_dir).unwrap();
    let source_one = source_dir.join("generated-one.png");
    let source_two = source_dir.join("generated-two.webp");
    fs::write(&source_one, b"one").unwrap();
    fs::write(&source_two, b"two").unwrap();

    let out_dir = temp.path().join("out");
    let item_one_dir = out_dir.join("item-0001");
    let item_two_dir = out_dir.join("item-0002");

    let item_one = write_generation_output_from_files(
        "first prompt",
        "gpt-image-2",
        &item_one_dir,
        &[source_one],
    )
    .expect("item one manifest should write");
    let item_two = write_generation_output_from_files(
        "second prompt",
        "gpt-image-2",
        &item_two_dir,
        &[source_two],
    )
    .expect("item two manifest should write");

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "first prompt\nsecond prompt\n").unwrap();

    let aggregate = write_batch_generation_manifest(
        &prompt_file,
        "gpt-image-2",
        &out_dir,
        &[item_one.clone(), item_two.clone()],
    )
    .expect("batch manifest should write");

    let root_manifest_path = out_dir.join("manifest.json");
    assert!(root_manifest_path.is_file(), "root manifest should exist");

    assert_eq!(aggregate.mode, "batch");
    assert_eq!(aggregate.prompt_file, prompt_file.to_string_lossy());
    assert_eq!(aggregate.model, "gpt-image-2");
    assert_eq!(
        aggregate.manifest_path,
        root_manifest_path.to_string_lossy()
    );
    assert_eq!(aggregate.item_count, 2);
    assert_eq!(aggregate.items.len(), 2);

    assert_eq!(aggregate.items[0].index, 1);
    assert_eq!(aggregate.items[0].prompt, "first prompt");
    assert_eq!(aggregate.items[0].out_dir, item_one_dir.to_string_lossy());
    assert_eq!(aggregate.items[0].manifest_path, item_one.manifest_path);
    assert_eq!(aggregate.items[0].images, item_one.images);
    assert_eq!(aggregate.items[0].response, item_one.response);

    assert_eq!(aggregate.items[1].index, 2);
    assert_eq!(aggregate.items[1].prompt, "second prompt");
    assert_eq!(aggregate.items[1].out_dir, item_two_dir.to_string_lossy());
    assert_eq!(aggregate.items[1].manifest_path, item_two.manifest_path);
    assert_eq!(aggregate.items[1].images, item_two.images);
    assert_eq!(aggregate.items[1].response, item_two.response);

    let manifest_text = fs::read_to_string(&root_manifest_path).unwrap();
    let manifest_json: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest_json["mode"], "batch");
    assert_eq!(
        manifest_json["prompt_file"],
        prompt_file.to_string_lossy().to_string()
    );
    assert_eq!(manifest_json["item_count"], 2);
    assert_eq!(manifest_json["items"][0]["index"], 1);
    assert_eq!(manifest_json["items"][0]["prompt"], "first prompt");
    assert_eq!(manifest_json["items"][1]["index"], 2);
    assert_eq!(manifest_json["items"][1]["prompt"], "second prompt");

    for forbidden in [
        "access-token",
        "refresh-token",
        "id-token",
        "Bearer",
        "b64_json",
    ] {
        assert!(
            !manifest_text.contains(forbidden),
            "root manifest should not contain {forbidden}"
        );
    }
}

#[test]
fn batch_output_empty_item_list_maps_to_response_contract_error() {
    let temp = tempdir().expect("tempdir should create");
    let out_dir = temp.path().join("out");
    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "prompt\n").unwrap();

    let err = write_batch_generation_manifest(&prompt_file, "gpt-image-2", &out_dir, &[])
        .expect_err("empty batch item list must fail");

    assert!(matches!(
        err,
        CliError::ImageGenerationResponseContract { .. }
    ));
}

#[test]
fn batch_output_missing_item_manifest_maps_to_filesystem_error() {
    let temp = tempdir().expect("tempdir should create");
    let source = temp.path().join("source.png");
    fs::write(&source, b"one").unwrap();

    let out_dir = temp.path().join("out");
    let item_dir = out_dir.join("item-0001");
    let item_manifest =
        write_generation_output_from_files("first", "gpt-image-2", &item_dir, &[source])
            .expect("item manifest should write");
    fs::remove_file(item_dir.join("manifest.json")).unwrap();

    let prompt_file = temp.path().join("prompts.txt");
    fs::write(&prompt_file, "first\n").unwrap();

    let err =
        write_batch_generation_manifest(&prompt_file, "gpt-image-2", &out_dir, &[item_manifest])
            .expect_err("missing item manifest should fail batch writer");

    assert!(matches!(err, CliError::OutputVerificationFailed));
    assert_eq!(
        err.error_envelope().error.code,
        "filesystem.output_write_failed"
    );
    assert!(
        !out_dir.join("manifest.json").exists(),
        "root manifest must not exist on failure"
    );
}

#[test]
fn batch_output_remove_stale_root_manifest_removes_existing_file() {
    let temp = tempdir().expect("tempdir should create");
    let out_dir = temp.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let root_manifest = out_dir.join("manifest.json");
    fs::write(&root_manifest, "stale").unwrap();

    remove_batch_manifest_if_exists(&out_dir).expect("stale root manifest removal should succeed");

    assert!(
        !root_manifest.exists(),
        "stale root manifest should be removed"
    );
}

#[test]
fn batch_output_remove_stale_root_manifest_ignores_not_found() {
    let temp = tempdir().expect("tempdir should create");
    let out_dir = temp.path().join("out");

    remove_batch_manifest_if_exists(&out_dir).expect("not-found stale root removal should succeed");
}

#[test]
fn batch_output_remove_stale_root_manifest_maps_non_file_failure_to_filesystem_error() {
    let temp = tempdir().expect("tempdir should create");
    let out_dir = temp.path().join("out");
    let root_manifest = out_dir.join("manifest.json");
    fs::create_dir_all(&root_manifest).unwrap();

    let err = remove_batch_manifest_if_exists(&out_dir)
        .expect_err("directory at root manifest path should fail stale removal");

    assert!(matches!(err, CliError::OutputWriteFailed));
    assert_eq!(
        err.error_envelope().error.code,
        "filesystem.output_write_failed"
    );
}
