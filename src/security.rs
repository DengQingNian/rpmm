//! 当前用户的本机命名管道访问控制，与 Windows 服务无关。
use crate::{
    Result,
    platform::win::{Handle, wide},
};

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows::{
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        System::Threading::*,
    },
    core::{PCWSTR, PWSTR},
};

pub struct Descriptor(pub PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    /// 释放系统分配的描述符。参数：无。返回：无。
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}
/// 解析 SDDL。参数：sddl 为安全描述符文本。返回：拥有所有权的描述符。
fn descriptor(sddl: &str) -> Result<Descriptor> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide(sddl).as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(crate::operation)?;
    }
    Ok(Descriptor(descriptor))
}
/// SID 转为字符串。参数：sid 为有效 SID 指针。返回：SID 文本。
fn sid_string(sid: PSID) -> Result<String> {
    unsafe {
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut text).map_err(crate::operation)?;
        let value = text.to_string().map_err(crate::operation);
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        value
    }
}
/// 查询当前进程账户 SID。参数：无。返回：SID 文本。
pub fn current_sid() -> Result<String> {
    unsafe {
        let mut raw = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw).map_err(crate::operation)?;
        let token = Handle(raw);
        let mut size = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut size);
        let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(data.as_mut_ptr().cast()),
            size,
            &mut size,
        )
        .map_err(crate::operation)?;
        sid_string((*(data.as_ptr() as *const TOKEN_USER)).User.Sid)
    }
}
/// 创建有显式 DACL 的仅本机管道。参数：name 为名称，first 为首实例标记。返回：异步服务端。
pub fn pipe(name: &str, first: bool) -> Result<NamedPipeServer> {
    let sid = current_sid()?;
    let descriptor = descriptor(&format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{sid})"))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, &attributes as *const _ as *mut _)
            .map_err(Into::into)
    }
}
