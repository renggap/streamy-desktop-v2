//! Secure credential storage using Windows Credential Manager via `keyring`.

const SERVICE_NAME: &str = "com.streamy.desktop";

pub fn set_password(profile_id: i64, password: &str) -> Result<(), String> {
    let username = format!("profile_{}", profile_id);
    let entry = keyring::Entry::new(SERVICE_NAME, &username)
        .map_err(|e| format!("Failed to create keyring entry: {:?}", e))?;
    entry
        .set_password(password)
        .map_err(|e| format!("Failed to set password in keyring: {:?}", e))?;
    Ok(())
}

pub fn get_password(profile_id: i64) -> Result<String, String> {
    let username = format!("profile_{}", profile_id);
    let entry = keyring::Entry::new(SERVICE_NAME, &username)
        .map_err(|e| format!("Failed to create keyring entry: {:?}", e))?;
    entry
        .get_password()
        .map_err(|e| format!("Failed to get password from keyring: {:?}", e))
}

pub fn delete_password(profile_id: i64) -> Result<(), String> {
    let username = format!("profile_{}", profile_id);
    let entry = keyring::Entry::new(SERVICE_NAME, &username)
        .map_err(|e| format!("Failed to create keyring entry: {:?}", e))?;
    let _ = entry.delete_credential();
    Ok(())
}
