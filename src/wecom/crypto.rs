use crate::config::schema::WeComConfig;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::{
    Engine as _,
    engine::GeneralPurpose,
    engine::{DecodePaddingMode, general_purpose},
};
use cbc::{Decryptor, Encryptor};
use rust_i18n::t;
use sha1::{Digest, Sha1};

type Aes256CbcEnc = Encryptor<aes::Aes256>;
type Aes256CbcDec = Decryptor<aes::Aes256>;

pub fn verify_signature(
    config: &WeComConfig,
    msg_signature: &str,
    timestamp: &str,
    nonce: &str,
    data: &str,
) -> anyhow::Result<()> {
    let Some(ref token) = config.token else {
        anyhow::bail!(t!("wecom_token_not_configured"));
    };

    let mut params = [token.as_str(), timestamp, nonce, data];
    params.sort_unstable();

    let mut hasher = Sha1::new();
    hasher.update(params.concat());
    let expected = hex::encode(hasher.finalize());

    if expected != msg_signature {
        anyhow::bail!(t!("invalid_wecom_signature"));
    }
    Ok(())
}

pub fn decrypt(config: &WeComConfig, encrypted: &str) -> anyhow::Result<String> {
    let Some(ref encoding_aes_key) = config.encoding_aes_key else {
        anyhow::bail!(t!("wecom_aes_key_not_configured"));
    };

    let alphabet = base64::alphabet::STANDARD;
    let b64_config = general_purpose::GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true);
    let engine = GeneralPurpose::new(&alphabet, b64_config);

    let key_to_decode = if encoding_aes_key.len() == 43 {
        format!("{encoding_aes_key}=")
    } else {
        encoding_aes_key.clone()
    };

    let aes_key_full = engine
        .decode(&key_to_decode)
        .or_else(|_| engine.decode(encoding_aes_key))?;

    if aes_key_full.len() < 32 {
        anyhow::bail!(t!("invalid_aes_key_length", length = aes_key_full.len()));
    }
    let aes_key = &aes_key_full[..32];

    let mut iv = [0u8; 16];
    iv.copy_from_slice(&aes_key[..16]);

    let mut ciphertext = engine
        .decode(encrypted.trim())
        .or_else(|_| engine.decode(encrypted.trim()))?;

    let decryptor = Aes256CbcDec::new(aes_key.into(), &iv.into());
    use aes::cipher::block_padding::NoPadding;
    let decrypted_raw = decryptor
        .decrypt_padded_mut::<NoPadding>(&mut ciphertext)
        .map_err(|e| anyhow::anyhow!(t!("aes_decryption_failed", error = e.to_string())))?;

    let padding_len = *decrypted_raw
        .last()
        .ok_or_else(|| anyhow::anyhow!(t!("empty_decrypted_buffer")))?
        as usize;
    if padding_len == 0 || padding_len > 32 {
        anyhow::bail!(t!("invalid_wecom_padding_length", length = padding_len));
    }
    let padding_start = decrypted_raw
        .len()
        .checked_sub(padding_len)
        .ok_or_else(|| anyhow::anyhow!(t!("padding_length_exceeds_buffer")))?;

    if !decrypted_raw[padding_start..]
        .iter()
        .all(|&b| b == padding_len as u8)
    {
        anyhow::bail!(t!("invalid_wecom_pkcs7_padding"));
    }
    let decrypted = &decrypted_raw[..padding_start];

    if decrypted.len() < 20 {
        anyhow::bail!(t!("decrypted_content_too_short", length = decrypted.len()));
    }

    let msg_len_bytes = &decrypted[16..20];
    let msg_len = u32::from_be_bytes([
        msg_len_bytes[0],
        msg_len_bytes[1],
        msg_len_bytes[2],
        msg_len_bytes[3],
    ]) as usize;

    if decrypted.len() < 20 + msg_len {
        anyhow::bail!(t!(
            "decrypted_msg_len_mismatch",
            buffer = decrypted.len(),
            msg_len = msg_len
        ));
    }

    let msg = &decrypted[20..20 + msg_len];
    let receive_id = &decrypted[20 + msg_len..];

    tracing::info!(
        "{}",
        t!(
            "wecom_decrypted_success",
            msg_len = msg_len,
            receive_id = String::from_utf8_lossy(receive_id)
        )
    );

    Ok(String::from_utf8_lossy(msg).to_string())
}

#[allow(dead_code)]
pub fn encrypt(config: &WeComConfig, plain_text: &str) -> anyhow::Result<String> {
    let Some(ref encoding_aes_key) = config.encoding_aes_key else {
        anyhow::bail!(t!("wecom_aes_key_not_configured"));
    };

    let alphabet = base64::alphabet::STANDARD;
    let b64_config = general_purpose::GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true);
    let engine = GeneralPurpose::new(&alphabet, b64_config);

    let key_to_decode = if encoding_aes_key.len() == 43 {
        format!("{encoding_aes_key}=")
    } else {
        encoding_aes_key.clone()
    };

    let aes_key_full = engine
        .decode(&key_to_decode)
        .or_else(|_| engine.decode(encoding_aes_key))?;

    if aes_key_full.len() < 32 {
        anyhow::bail!(t!("invalid_aes_key_length", length = aes_key_full.len()));
    }
    let aes_key = &aes_key_full[..32];

    let mut iv = [0u8; 16];
    iv.copy_from_slice(&aes_key[..16]);

    let random_bytes: [u8; 16] = rand::random();
    let msg_bytes = plain_text.as_bytes();
    let msg_len = msg_bytes.len() as u32;
    let receive_id = "";

    let mut data = Vec::with_capacity(20 + msg_bytes.len() + receive_id.len());
    data.extend_from_slice(&random_bytes);
    data.extend_from_slice(&msg_len.to_be_bytes());
    data.extend_from_slice(msg_bytes);
    data.extend_from_slice(receive_id.as_bytes());

    let padding_len = 32 - (data.len() % 32);
    data.extend(std::iter::repeat(padding_len as u8).take(padding_len));

    let data_len = data.len();
    let mut buffer = data;
    let encryptor = Aes256CbcEnc::new(aes_key.into(), &iv.into());
    use aes::cipher::block_padding::NoPadding;
    let ciphertext = encryptor
        .encrypt_padded_mut::<NoPadding>(&mut buffer, data_len)
        .map_err(|e| anyhow::anyhow!(t!("aes_encryption_failed", error = e.to_string())))?;

    Ok(engine.encode(ciphertext))
}
