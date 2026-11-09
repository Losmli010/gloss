//! 凭据管理器（凭据应用编程接口）的安全封装：通用凭据的读、写、删。
//!
//! 凭据以目标名（服务名加键串）寻址，载荷按原始字节存取——密钥半边
//! （凭据句柄模块）负责字符串编解码与缓存，本模块只面对系统接口。
//! 密钥红线：载荷不进日志、不进错误消息，错误只报告系统侧的错误码与
//! 系统消息；条目不存在不是错误（读取回空、删除恒成功），语义与核心层
//! 端口契约一致。

use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};
use windows::core::HSTRING;
use windows::core::PWSTR;

use gloss_core::model::GlossError;

pub(crate) fn read(target: &str) -> Result<Option<Vec<u8>>, GlossError> {
    let wide = HSTRING::from(target);
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `wide` lives through the call and the credential store copies the
    // target name while serving the lookup; the out pointer is released by
    // `CredFree` below on every path after a successful call.
    let outcome = unsafe { CredReadW(&wide, CRED_TYPE_GENERIC, None, &mut credential) };
    let record = match outcome {
        Ok(()) => {
            // SAFETY: a successful `CredReadW` always hands back a valid,
            // non-null credential allocated by the credential store, owned by
            // this call until `CredFree` releases it.
            unsafe { *credential }
        }
        Err(err) if err.code() == ERROR_NOT_FOUND_HRESULT => return Ok(None),
        Err(err) => {
            return Err(GlossError::Config(format!(
                "credential manager read failed: {err}"
            )));
        }
    };
    let blob = copy_blob(&record);
    // SAFETY: `credential` came from the successful `CredReadW` above and is
    // freed exactly once here, after the payload bytes have been copied out.
    unsafe { CredFree(credential.cast()) };
    Ok(Some(blob))
}

pub(crate) fn write(target: &str, blob: &[u8]) -> Result<(), GlossError> {
    let wide = HSTRING::from(target);
    let blob_size = u32::try_from(blob.len())
        .map_err(|_| GlossError::Config("credential payload exceeds the system limit".into()))?;
    let record = CREDENTIALW {
        Flags: Default::default(),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(wide.as_ptr() as *mut u16),
        Comment: PWSTR::null(),
        LastWritten: Default::default(),
        CredentialBlobSize: blob_size,
        CredentialBlob: blob.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR::null(),
    };
    // SAFETY: `record` borrows only `wide` and `blob`, both alive for the
    // whole call; the credential store copies the record on success.
    let outcome = unsafe { CredWriteW(&record, 0) };
    outcome.map_err(|err| GlossError::Config(format!("credential manager write failed: {err}")))
}

pub(crate) fn delete(target: &str) -> Result<(), GlossError> {
    let wide = HSTRING::from(target);
    // SAFETY: `wide` lives through the call; deletion takes only the target
    // name and returns no pointer.
    let outcome = unsafe { CredDeleteW(&wide, CRED_TYPE_GENERIC, None) };
    match outcome {
        Ok(()) => Ok(()),
        Err(err) if err.code() == ERROR_NOT_FOUND_HRESULT => Ok(()),
        Err(err) => Err(GlossError::Config(format!(
            "credential manager delete failed: {err}"
        ))),
    }
}

fn copy_blob(record: &CREDENTIALW) -> Vec<u8> {
    if record.CredentialBlob.is_null() || record.CredentialBlobSize == 0 {
        return Vec::new();
    }
    // SAFETY: the blob pointer with its size describes the payload owned by
    // the record returned by `CredReadW`; the copy happens before `CredFree`
    // frees the record, and the span stays within `CredentialBlobSize`.
    let bytes = unsafe {
        std::slice::from_raw_parts(record.CredentialBlob, record.CredentialBlobSize as usize)
    };
    bytes.to_vec()
}

const ERROR_NOT_FOUND_HRESULT: windows::core::HRESULT =
    windows::core::HRESULT::from_win32(ERROR_NOT_FOUND.0);
