use aes_gcm_siv::{
    aead::{generic_array::GenericArray, rand_core::RngCore, Aead, OsRng},
    Aes256GcmSiv, KeyInit, Nonce,
};
use argon2::Config;

use crate::prelude::*;

#[derive(serde::Serialize, serde::Deserialize)]
struct PrecryptorFile {
    data: Vec<u8>,
    nonce: [u8; 12],
    salt: [u8; 32],
}

/// Encrypts some data using a password, internally also a nonce and salt.
/// Uses a secure AES256-GCM-SIV algorithm (very safe in 2024).
///
/// # Examples
///
/// ```no_run
/// use bitbazaar::encrypt;
///
/// let encrypted_data = encrypt::encrypt_aes256(b"example text", b"example password").expect("Failed to encrypt");
/// // and now you can write it to a file:
/// // fs::write("encrypted_text.txt", encrypted_data).expect("Failed to write to file");
/// ```
///
pub fn encrypt_aes256(data: &[u8], password: &[u8]) -> RResult<Vec<u8>, AnyErr> {
    // Generating salt:
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);
    let config = Config {
        hash_length: 32,
        ..Default::default()
    };

    // Generating key:
    let password = argon2::hash_raw(password, &salt, &config).change_context(AnyErr)?;

    let key = GenericArray::from_slice(&password);
    let cipher = Aes256GcmSiv::new(key);

    // Generating nonce:
    let mut nonce_rand = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_rand);
    let nonce = Nonce::from_slice(&nonce_rand);

    // Encrypting:
    let ciphertext = match cipher.encrypt(nonce, data.as_ref()) {
        Ok(ciphertext) => ciphertext,
        Err(_) => return Err(anyerr!("Failed to encrypt data -> invalid password")),
    };

    let file = PrecryptorFile {
        data: ciphertext,
        nonce: nonce_rand,
        salt,
    };

    // Encoding:
    let encoded: Vec<u8> = bincode::serialize(&file).change_context(AnyErr)?;

    Ok(encoded)
}

/// Decrypts some data and returns the result
///
/// # Examples
///
/// ```no_run
/// use bitbazaar::encrypt;
///
/// let encrypted_data = encrypt::encrypt_aes256(b"example text", b"example password").expect("Failed to encrypt");
///
/// let data = encrypt::decrypt_aes256(&encrypted_data, b"example password").expect("Failed to decrypt");
/// // and now you can print it to stdout:
/// // println!("data: {}", String::from_utf8(data.clone()).expect("Data is not a utf8 string"));
/// // or you can write it to a file:
/// // fs::write("text.txt", data).expect("Failed to write to file");
/// ```
///
pub fn decrypt_aes256(data: &[u8], password: &[u8]) -> RResult<Vec<u8>, AnyErr> {
    // Decoding:
    let decoded: PrecryptorFile = bincode::deserialize(data).change_context(AnyErr)?;

    let config = Config {
        hash_length: 32,
        ..Default::default()
    };

    // Generating key:
    let password = argon2::hash_raw(password, &decoded.salt, &config).change_context(AnyErr)?;

    let key = GenericArray::from_slice(&password);
    let cipher = Aes256GcmSiv::new(key);
    let nonce = Nonce::from_slice(&decoded.nonce);

    // Decrypting:
    let text = match cipher.decrypt(nonce, decoded.data.as_ref()) {
        Ok(ciphertext) => ciphertext,
        Err(_) => return Err(anyerr!("Failed to encrypt data -> invalid password")),
    };

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_aes256() -> RResult<(), AnyErr> {
        let data = b"example text";
        let password_1 = b"example password";
        let password_2 = b"example password 2";

        // 1. Simple encrypt/decrypt works.
        let encrypted_data = encrypt_aes256(data, password_1)?;
        let decrypted_data = decrypt_aes256(&encrypted_data, password_1)?;
        assert_eq!(data, decrypted_data.as_slice());

        // 2. Wrong password fails to decrypt.
        let decrypted_data = decrypt_aes256(&encrypted_data, password_2);
        assert!(decrypted_data.is_err());

        // 3. Different passwords using same content leads to different encrypted data.
        let encrypted_data_2 = encrypt_aes256(data, password_2)?;
        assert_ne!(encrypted_data, encrypted_data_2);
        assert_eq!(
            data,
            decrypt_aes256(&encrypted_data_2, password_2)?.as_slice()
        );

        Ok(())
    }
}
