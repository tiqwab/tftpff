use anyhow::{Context, Result};
use nix;
use std::ffi::CString;
use std::path::Path;

pub fn chmod(path: impl AsRef<Path>, mode: u32) -> Result<()> {
    let path = CString::new(path.as_ref().to_string_lossy().to_string())?;
    let res = unsafe { nix::libc::chmod(path.as_ptr(), mode) };
    nix::errno::Errno::result(res).map(drop)?;

    Ok(())
}

pub fn chown(path: impl AsRef<Path>, user: &str, group: &str) -> Result<()> {
    let group = nix::unistd::Group::from_name(group)?
        .with_context(|| format!("group is not found: {}", group))?;
    let user = nix::unistd::User::from_name(user)?
        .with_context(|| format!("user is not found: {}", user))?;
    nix::unistd::chown(path.as_ref(), Some(user.uid), Some(group.gid))?;

    Ok(())
}

pub fn drop_privilege(user: &str, group: &str) -> Result<()> {
    // should drop group privilege first
    // ref. https://wiki.sei.cmu.edu/confluence/display/c/POS36-C.+Observe+correct+revocation+order+while+relinquishing+privileges
    let group = nix::unistd::Group::from_name(group)?
        .with_context(|| format!("group is not found: {}", group))?;
    nix::unistd::setgid(group.gid)?;

    let user = nix::unistd::User::from_name(user)?
        .with_context(|| format!("user is not found: {}", user))?;
    nix::unistd::setuid(user.uid)?;

    Ok(())
}
