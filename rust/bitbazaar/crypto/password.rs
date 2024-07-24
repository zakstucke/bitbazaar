use argon2::password_hash::{PasswordHasher, PasswordVerifier};

use crate::prelude::*;

/// Hash a password to a "PHC string" for intermediary password storage.
///
/// Uses argon2, should be very secure.
pub fn password_hash_argon2id_19(password: &str) -> RResult<String, AnyErr> {
    // https://docs.rs/argon2/0.5.3/argon2/#password-hashing
    let salt =
        argon2::password_hash::SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let argon2 = argon2id_19_config();
    Ok(argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyerr!("Failed to hash password: {:?}", e))?
        .to_string())
}

/// Verify an entered password matches a stored "PHC string" password hash.
///
/// Uses argon2, should be very secure.
pub fn password_verify_argon2id_19(
    entered_pswd: &str,
    real_pswd_hash: &str,
) -> RResult<bool, AnyErr> {
    // https://docs.rs/argon2/0.5.3/argon2/#password-hashing
    let parsed_hash = argon2::password_hash::PasswordHash::new(real_pswd_hash)
        .map_err(|e| anyerr!("Failed to parse password hash: {:?}", e))?;
    match argon2id_19_config().verify_password(entered_pswd.as_bytes(), &parsed_hash) {
        Ok(_noop) => Ok(true),
        Err(e) => match e {
            argon2::password_hash::Error::Password => Ok(false),
            _ => Err(anyerr!("Failed to verify password: {:?}", e)),
        },
    }
}

fn argon2id_19_config() -> argon2::Argon2<'static> {
    // These are the defaults currently, just future proofing if they change.
    argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        // This is v19 in hex.
        argon2::Version::V0x13,
        // Keeping default as don't think this will break hashes.
        argon2::Params::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hash_argon2id_19() -> RResult<(), AnyErr> {
        let pswd = "password";
        let hash = password_hash_argon2id_19(pswd)?;
        assert_ne!(pswd, &hash);
        assert!(password_verify_argon2id_19(pswd, &hash)?);
        assert!(!password_verify_argon2id_19("wrong", &hash)?);

        let pswd_2 = "password2";
        let hash_2 = password_hash_argon2id_19(pswd_2)?;
        assert_ne!(pswd, &hash_2);
        assert_ne!(hash, hash_2);
        assert!(password_verify_argon2id_19(pswd_2, &hash_2)?);
        assert!(!password_verify_argon2id_19("wrong", &hash_2)?);

        // Hashing the same password twice should produce 2 different hashes because of the in-built salt:
        let hash_3 = password_hash_argon2id_19(pswd)?;
        assert_ne!(hash, hash_3);
        assert!(password_verify_argon2id_19(pswd, &hash)?);
        assert!(password_verify_argon2id_19(pswd, &hash_3)?);

        Ok(())
    }
}
