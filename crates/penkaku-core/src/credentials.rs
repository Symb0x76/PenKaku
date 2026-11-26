//! "Remember me" for the ChiXie account: the password lives only in the OS credential store
//! (Windows Credential Manager, via `keyring`). Settings keep the account (phone number) but
//! never the password. ChiXie3 itself stores the password in plaintext in
//! `%USERPROFILE%\ChiXie3\conf\config.ini [LoginConfig]`; we do not copy that.

use crate::{Error, Result};

/// Credential Manager target name prefix; entries show up as `penkaku` / `<account>`.
const SERVICE: &str = "penkaku";

fn entry(account: &str) -> Result<keyring::Entry> {
    entry_in(SERVICE, account)
}

/// Where versions before the rename to PenKaku saved the password.
fn legacy_entry(account: &str) -> Result<keyring::Entry> {
    entry_in(crate::LEGACY_NAME, account)
}

fn entry_in(service: &str, account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(service, account).map_err(|e| Error::Credential(e.to_string()))
}

/// Store (or replace) the password for `account`.
pub fn save(account: &str, password: &str) -> Result<()> {
    entry(account)?.set_password(password).map_err(|e| Error::Credential(e.to_string()))
}

/// The stored password, or `None` if nothing was saved for `account`. A password saved before
/// the rename is moved to the new entry on first load.
pub fn load(account: &str) -> Result<Option<String>> {
    if let Some(p) = get(&entry(account)?)? {
        return Ok(Some(p));
    }
    let legacy = legacy_entry(account)?;
    let Some(p) = get(&legacy)? else { return Ok(None) };
    save(account, &p)?;
    delete(&legacy)?;
    Ok(Some(p))
}

/// Remove the stored password (and any pre-rename copy); succeeds if there was none.
pub fn forget(account: &str) -> Result<()> {
    delete(&entry(account)?)?;
    delete(&legacy_entry(account)?)
}

fn get(entry: &keyring::Entry) -> Result<Option<String>> {
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(Error::Credential(e.to_string())),
    }
}

fn delete(entry: &keyring::Entry) -> Result<()> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::Credential(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real OS credential store, so it is opt-in:
    /// `cargo test -p chixie-core credentials -- --ignored`.
    #[test]
    #[ignore = "writes to the OS credential store"]
    fn roundtrip_in_os_store() {
        let account = format!("test-{}", std::process::id());
        forget(&account).unwrap();
        assert_eq!(load(&account).unwrap(), None);
        save(&account, "s3cret-测试").unwrap();
        assert_eq!(load(&account).unwrap().as_deref(), Some("s3cret-测试"));
        forget(&account).unwrap();
        assert_eq!(load(&account).unwrap(), None);
        forget(&account).unwrap(); // idempotent
    }
}
