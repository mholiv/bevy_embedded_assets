#![cfg(all(feature = "compression", feature = "encryption"))]
//! Run with `BEVY_EMBEDDED_ASSETS_KEY=TEST_KEY`.

use std::fmt::Display;

use bevy::{
    asset::{LoadContext, io::Reader},
    prelude::*,
};
use bevy_embedded_assets::{EmbeddedAssetPlugin, EmbeddedAssetReader, PluginMode};
use thiserror::Error;

#[derive(Asset, TypePath, Debug)]
pub struct TestAsset {
    pub value: String,
}

#[derive(Default, TypePath)]
pub struct TestAssetLoader;

#[derive(Debug, Error)]
pub struct TestError;

impl Display for TestError {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl bevy::asset::AssetLoader for TestAssetLoader {
    type Asset = TestAsset;
    type Settings = ();
    type Error = TestError;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        bevy::asset::AsyncReadExt::read_to_end(reader, &mut bytes)
            .await
            .unwrap();

        Ok(TestAsset {
            value: String::from_utf8(bytes).unwrap(),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["test"]
    }
}

const KEY: &str = "TEST_KEY";

fn check(app: &mut App, prefix: &str) {
    app.init_asset::<TestAsset>()
        .init_asset_loader::<TestAssetLoader>();
    app.finish();

    let asset_server = app.world_mut().resource_mut::<AssetServer>();
    let handles: Vec<Handle<TestAsset>> = [
        "example_asset.test",
        "açèt.test",
        "subdir/other_asset.test",
        "compressible.test",
    ]
    .iter()
    .map(|path| asset_server.load(format!("{prefix}{path}")))
    .collect();
    app.update();
    let test_assets = app.world_mut().resource_mut::<Assets<TestAsset>>();
    let values: Vec<_> = handles
        .iter()
        .map(|h| test_assets.get(h).unwrap().value.clone())
        .collect();
    assert_eq!(values[0], "hello");
    assert_eq!(values[1], "with special chars");
    assert_eq!(values[2], "in subdirectory");
    assert_eq!(values[3], "compress me! ".repeat(64));
}

#[test]
fn auto_load_decodes() {
    let mut app = App::new();
    app.add_plugins(EmbeddedAssetPlugin {
        encryption_key: Some(KEY.to_string()),
        ..default()
    })
    .add_plugins(DefaultPlugins);
    check(&mut app, "embedded://");
}

#[test]
fn replace_default_decodes() {
    let mut app = App::new();
    app.add_plugins(EmbeddedAssetPlugin {
        mode: PluginMode::ReplaceDefault,
        encryption_key: Some(KEY.to_string()),
    })
    .add_plugins(DefaultPlugins.set(AssetPlugin {
        file_path: "test".to_string(),
        ..default()
    }));
    check(&mut app, "");
}

#[test]
fn reader_needs_right_key() {
    let path = std::path::Path::new("compressible.test");
    let reader = EmbeddedAssetReader::preloaded();
    assert!(reader.load_bytes_sync(path).is_err());
    let reader = EmbeddedAssetReader::preloaded().with_key("WRONG_KEY");
    assert!(reader.load_bytes_sync(path).is_err());
    let reader = EmbeddedAssetReader::preloaded().with_key(KEY);
    assert_eq!(
        reader.load_bytes_sync(path).unwrap(),
        "compress me! ".repeat(64).as_bytes()
    );
    assert!(reader.load_path_sync(path).is_err());
}
