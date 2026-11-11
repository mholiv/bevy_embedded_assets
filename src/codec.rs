//! Decoding of assets compressed or encrypted by `build.rs`.

use std::{borrow::Cow, io};

pub(crate) type Key = [u8; 32];

/// Same derivation as `parse_key` in `build.rs`.
#[cfg(feature = "encryption")]
pub(crate) fn derive_key(passphrase: &str) -> Key {
    blake3::derive_key(
        "bevy_embedded_assets asset encryption v1",
        passphrase.as_bytes(),
    )
}

/// Decrypts then decompresses. Assets stored raw are borrowed, not copied.
#[allow(unused_variables, unused_mut, clippy::unnecessary_wraps)]
pub(crate) fn decode(
    bytes: &'static [u8],
    compressed: bool,
    key: Option<&Key>,
) -> io::Result<Cow<'static, [u8]>> {
    let mut data = Cow::Borrowed(bytes);

    #[cfg(feature = "encryption")]
    {
        use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
        let key = key.ok_or_else(|| io::Error::other("no encryption key configured"))?;
        if data.len() < 12 {
            return Err(io::Error::other("encrypted asset is truncated"));
        }
        let (nonce, sealed) = data.split_at(12);
        data = Cow::Owned(
            ChaCha20Poly1305::new(key.into())
                .decrypt(Nonce::from_slice(nonce), sealed)
                .map_err(|_| io::Error::other("decryption failed: wrong key or corrupt asset"))?,
        );
    }

    #[cfg(feature = "compression")]
    if compressed {
        let mut out = Vec::new();
        io::Read::read_to_end(
            &mut structured_zstd::decoding::StreamingDecoder::new(&data[..]),
            &mut out,
        )?;
        data = Cow::Owned(out);
    }

    Ok(data)
}
