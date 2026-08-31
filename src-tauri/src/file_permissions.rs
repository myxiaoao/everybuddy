use std::{fs, path::Path};

pub fn create_private_file(path: &Path) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?;
    secure_path(path)
}

#[cfg(unix)]
pub fn secure_path(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
pub fn secure_path(path: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC};

    let file = fs::OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .open(path)?;
    secure_file(&file, 0)
}

#[cfg(not(any(unix, windows)))]
pub fn secure_path(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
pub fn secure_directory(path: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::{
        Security::{CONTAINER_INHERIT_ACE, OBJECT_INHERIT_ACE},
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, READ_CONTROL, WRITE_DAC},
    };

    let directory = fs::OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    secure_file(&directory, OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE)
}

#[cfg(unix)]
pub fn secure_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(any(unix, windows)))]
pub fn secure_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn secure_file(file: &fs::File, ace_flags: u32) -> std::io::Result<()> {
    use std::{ffi::c_void, mem::size_of, os::windows::io::AsRawHandle, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, GENERIC_ALL, HANDLE},
        Security::{
            AddAccessAllowedAceEx,
            Authorization::{SetSecurityInfo, SE_FILE_OBJECT},
            GetLengthSid, GetTokenInformation, InitializeAcl, TokenUser, ACCESS_ALLOWED_ACE, ACL,
            ACL_REVISION, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
            TOKEN_QUERY, TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token: HANDLE = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(windows_permission_error(None));
    }

    let result = (|| {
        let mut token_info_size = 0;
        unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut token_info_size) };
        if token_info_size == 0 {
            return Err(windows_permission_error(None));
        }

        // usize storage keeps the buffer aligned for TOKEN_USER.
        let mut token_info = vec![0usize; (token_info_size as usize).div_ceil(size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                token_info.as_mut_ptr().cast::<c_void>(),
                token_info_size,
                &mut token_info_size,
            )
        } == 0
        {
            return Err(windows_permission_error(None));
        }

        let user = unsafe { &*token_info.as_ptr().cast::<TOKEN_USER>() };
        let sid_length = unsafe { GetLengthSid(user.User.Sid) } as usize;
        if sid_length == 0 {
            return Err(windows_permission_error(None));
        }

        let acl_size =
            size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() + sid_length - size_of::<u32>();
        let mut acl_storage = vec![0u32; acl_size.div_ceil(size_of::<u32>())];
        let acl = acl_storage.as_mut_ptr().cast::<ACL>();
        if unsafe { InitializeAcl(acl, acl_size as u32, ACL_REVISION) } == 0
            || unsafe {
                AddAccessAllowedAceEx(acl, ACL_REVISION, ace_flags, GENERIC_ALL, user.User.Sid)
            } == 0
        {
            return Err(windows_permission_error(None));
        }

        let status = unsafe {
            SetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            )
        };
        if status != 0 {
            return Err(windows_permission_error(Some(status)));
        }
        Ok(())
    })();

    unsafe { CloseHandle(token) };
    result
}

#[cfg(windows)]
fn windows_permission_error(status: Option<u32>) -> std::io::Error {
    status
        .map(|code| std::io::Error::from_raw_os_error(code as i32))
        .unwrap_or_else(std::io::Error::last_os_error)
}
