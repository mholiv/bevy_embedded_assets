use std::{
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

const ASSET_PATH_VAR: &str = "BEVY_ASSET_PATH";
const KEY_VAR: &str = "BEVY_EMBEDDED_ASSETS_KEY";
const LEVEL_VAR: &str = "BEVY_EMBEDDED_ASSETS_ZSTD_LEVEL";
const SKIP_VAR: &str = "BEVY_EMBEDDED_ASSETS_COMPRESS_SKIP";
#[cfg(feature = "compression")]
/// Formats that are already compressed.
const DEFAULT_SKIP: &str =
    "png,jpg,jpeg,webp,ktx2,basis,dds,glb,ogg,mp3,flac,opus,m4a,mp4,webm,zip,gz,zst,br,woff2";

fn main() {
    cargo_emit::rerun_if_env_changed!(ASSET_PATH_VAR);
    cargo_emit::rerun_if_env_changed!(KEY_VAR);
    cargo_emit::rerun_if_env_changed!(LEVEL_VAR);
    cargo_emit::rerun_if_env_changed!(SKIP_VAR);

    // Check if env variable is set for the assets folder
    if let Some(dir) = env::var(ASSET_PATH_VAR)
        .ok()
        .map(|v| Path::new(&v).to_path_buf())
        .and_then(|path| {
            if path.exists() {
                Some(path)
            } else {
                cargo_emit::warning!(
                    "${} points to an unknown folder: {}",
                    ASSET_PATH_VAR,
                    path.to_string_lossy()
                );
                None
            }
        })
        // Otherwise, search for the target folder and look for an assets folder next to it
        .or_else(|| {
            env::var("OUT_DIR")
                .ok()
                .map(|v| Path::new(&v).to_path_buf())
                .and_then(|path| {
                    for ancestor in path.ancestors() {
                        if let Some(last) = ancestor.file_name()
                            && last == "target"
                        {
                            return ancestor.parent().map(|parent| {
                                let imported_dir = parent.join("imported_assets");
                                if imported_dir.exists() {
                                    imported_dir.join("Default")
                                } else {
                                    parent.join("assets")
                                }
                            });
                        }
                    }
                    None
                })
                .and_then(|path| {
                    if path.exists() {
                        Some(path)
                    } else {
                        cargo_emit::warning!(
                            "Could not find asset folder from Cargo build directory"
                        );
                        None
                    }
                })
        })
    {
        cargo_emit::rerun_if_changed!(dir.to_string_lossy());
        cargo_emit::warning!("Asset folder found: {}", dir.to_string_lossy());

        let out_dir = env::var_os("OUT_DIR").unwrap();
        let dest_path = Path::new(&out_dir).join("include_all_assets.rs");

        let mut file = File::create(dest_path).unwrap();
        file.write_all(
            "/// Generated function that will embed all assets.
#[allow(unused_variables, unused_qualifications, clippy::non_ascii_literal)]
fn include_all_assets(mut registry: impl EmbeddedRegistry){\n"
                .as_ref(),
        )
        .unwrap();

        let building_for_not_windows =
            std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|v| v != "windows");

        visit_dirs(&dir)
            .iter()
            .map(|path| (path, path.strip_prefix(&dir).unwrap()))
            .enumerate()
            .for_each(|(index, (fullpath, path))| {
                let mut path = path.to_string_lossy().to_string();
                if building_for_not_windows {
                    // replace paths with forward slash in case we're building from windows
                    path = path.replace(std::path::MAIN_SEPARATOR, "/");
                }
                cargo_emit::rerun_if_changed!(fullpath.to_string_lossy());
                let (included, compressed) = encode_asset(index, &path, fullpath, &out_dir);
                file.write_all(
                    format!(
                        r#"    registry.insert_included_asset({:?}, include_bytes!({:?}), {});
"#,
                        path,
                        included.to_string_lossy(),
                        compressed
                    )
                    .as_ref(),
                )
                .unwrap();
            });

        file.write_all("}".as_ref()).unwrap();
    } else if std::env::var("DOCS_RS").is_ok() {
        let out_dir = env::var_os("OUT_DIR").unwrap();
        let dest_path = Path::new(&out_dir).join("include_all_assets.rs");

        let mut file = File::create(dest_path).unwrap();
        file.write_all(
            "/// Generated function that will embed all assets.
#[allow(unused_variables, unused_qualifications, clippy::non_ascii_literal)]
fn include_all_assets(registry: impl EmbeddedRegistry){}"
                .as_ref(),
        )
        .unwrap();
    } else {
        cargo_emit::warning!(
            "Could not find asset folder, please specify its path with ${}",
            ASSET_PATH_VAR
        );
        panic!("No asset folder found");
    }
}

/// Returns the file to embed and whether it is compressed.
fn encode_asset(
    index: usize,
    name: &str,
    fullpath: &Path,
    out_dir: &std::ffi::OsStr,
) -> (PathBuf, bool) {
    if !cfg!(any(feature = "compression", feature = "encryption")) {
        return (fullpath.to_path_buf(), false);
    }
    #[allow(unused_mut)]
    let mut data = fs::read(fullpath).unwrap();
    #[allow(unused_mut)]
    let mut compressed = false;

    #[cfg(feature = "compression")]
    {
        let skip = env::var(SKIP_VAR).unwrap_or_else(|_| DEFAULT_SKIP.to_string());
        let ext = fullpath
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !skip.split(',').any(|s| s.trim().eq_ignore_ascii_case(&ext)) {
            use structured_zstd::{decoding::StreamingDecoder, encoding};
            let level = env::var(LEVEL_VAR).map_or(19, |l| {
                l.parse()
                    .ok()
                    .filter(|l| (1..=22).contains(l))
                    .unwrap_or_else(|| panic!("${LEVEL_VAR} must be 1-22, got {l:?}"))
            });
            let packed =
                encoding::compress_to_vec(&data[..], encoding::CompressionLevel::from_level(level));
            let mut check = Vec::with_capacity(data.len());
            std::io::Read::read_to_end(&mut StreamingDecoder::new(&packed[..]), &mut check)
                .unwrap();
            assert!(
                check == data,
                "zstd round-trip failed for {}",
                fullpath.display()
            );
            // Small or incompressible files can grow.
            if packed.len() < data.len() {
                data = packed;
                compressed = true;
            }
        }
    }

    #[cfg(feature = "encryption")]
    {
        use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
        let key = parse_key();
        // Nonce derived from path and content so it's never reused and builds are reproducible.
        let mut hasher = blake3::Hasher::new_keyed(&key);
        hasher.update(name.as_bytes()).update(&[0]).update(&data);
        let nonce = *Nonce::from_slice(&hasher.finalize().as_bytes()[..12]);
        let sealed = ChaCha20Poly1305::new(&key.into())
            .encrypt(&nonce, &data[..])
            .unwrap();
        data = [nonce.as_slice(), &sealed].concat();
    }
    #[cfg(not(feature = "encryption"))]
    let _ = name;

    let dir = Path::new(out_dir).join("embedded");
    fs::create_dir_all(&dir).unwrap();
    let out = dir.join(format!("{index}.bin"));
    fs::write(&out, data).unwrap();
    (out, compressed)
}

#[cfg(feature = "encryption")]
fn parse_key() -> [u8; 32] {
    let passphrase = env::var(KEY_VAR)
        .ok()
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| panic!("feature `encryption` requires ${KEY_VAR} (any passphrase)"));
    // Same derivation as `codec::derive_key`.
    blake3::derive_key(
        "bevy_embedded_assets asset encryption v1",
        passphrase.as_bytes(),
    )
}

fn visit_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut collected = vec![];
    if dir.is_dir() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                collected.append(&mut visit_dirs(&path));
            } else {
                collected.push(path);
            }
        }
    }
    collected
}
