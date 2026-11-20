use crate::error::AppError;
use crate::transcription::TranscriptionProvider;
use keyring_core::api::CredentialStoreApi;

const SERVICE: &str = "com.voxely.desktop";
fn entry(provider: TranscriptionProvider) -> Result<keyring_core::Entry, AppError> {
    // The Windows store's default target remains user.service, as in keyring 3.
    windows_native_keyring_store::Store::new()
        .and_then(|store| store.build(SERVICE, &format!("{}-api-key", provider.id()), None))
        .map_err(|e| AppError::StorageFailed(e.to_string()))
}

pub fn set_api_key(provider: TranscriptionProvider, key: &str) -> Result<(), AppError> {
    entry(provider)?
        .set_password(key.trim())
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    Ok(())
}

pub fn get_api_key() -> Result<Option<String>, AppError> {
    get_provider_key(TranscriptionProvider::OpenRouter)
}

pub fn get_provider_key(provider: TranscriptionProvider) -> Result<Option<String>, AppError> {
    match entry(provider)?.get_password() {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::StorageFailed(e.to_string())),
    }
}

pub fn has_api_key(provider: TranscriptionProvider) -> bool {
    matches!(get_provider_key(provider), Ok(Some(_)))
}

pub fn delete_api_key(provider: TranscriptionProvider) -> Result<(), AppError> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::StorageFailed(e.to_string())),
    }
}
