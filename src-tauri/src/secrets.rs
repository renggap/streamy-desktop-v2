//! Secure credential storage using Windows Credential Manager via `keyring`.
//! Two slots per profile: source password (`profile_N`) and DJ password
//! (`profile_N_dj`).

const SERVICE_NAME: &str = "com.streamy.desktop";

fn set_slot(username: &str, password: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE_NAME, username)
        .map_err(|e| format!("Failed to create keyring entry: {:?}", e))?;
    entry
        .set_password(password)
        .map_err(|e| format!("Failed to set password in keyring: {:?}", e))?;
    Ok(())
}

fn get_slot(username: &str) -> Result<String, String> {
    let entry = keyring::Entry::new(SERVICE_NAME, username)
        .map_err(|e| format!("Failed to create keyring entry: {:?}", e))?;
    entry
        .get_password()
        .map_err(|e| format!("Failed to get password from keyring: {:?}", e))
}

fn delete_slot(username: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE_NAME, username) {
        let _ = entry.delete_credential();
    }
}

pub fn set_password(profile_id: i64, password: &str) -> Result<(), String> {
    set_slot(&format!("profile_{profile_id}"), password)
}

pub fn get_password(profile_id: i64) -> Result<String, String> {
    get_slot(&format!("profile_{profile_id}"))
}

pub fn set_dj_password(profile_id: i64, password: &str) -> Result<(), String> {
    set_slot(&format!("profile_{profile_id}_dj"), password)
}

pub fn get_dj_password(profile_id: i64) -> Result<String, String> {
    get_slot(&format!("profile_{profile_id}_dj"))
}

pub fn delete_password(profile_id: i64) -> Result<(), String> {
    delete_slot(&format!("profile_{profile_id}"));
    delete_slot(&format!("profile_{profile_id}_dj"));
    Ok(())
}
