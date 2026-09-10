use crate::error::AppError;

const SERVICE: &str = "com.voxely.desktop";
const USER: &str = "openrouter-api-key";

pub fn set_api_key(key: &str) -> Result<(), AppError> {
    let entry =
        keyring::Entry::new(SERVICE, USER).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    entry
        .set_password(key.trim())
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    Ok(())
}

pub fn get_api_key() -> Result<Option<String>, AppError> {
    let entry =
        keyring::Entry::new(SERVICE, USER).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    match entry.get_password() {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::StorageFailed(e.to_string())),
    }
}

pub fn has_api_key() -> bool {
    matches!(get_api_key(), Ok(Some(_)))
}

pub fn delete_api_key() -> Result<(), AppError> {
    let entry =
        keyring::Entry::new(SERVICE, USER).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::StorageFailed(e.to_string())),
    }
}
