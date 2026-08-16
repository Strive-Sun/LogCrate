use super::{MftEnumeration, MftRecord, UsnJournalInfo, UsnReadSummary};
use anyhow::{anyhow, bail, Context};
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, sleep};
use std::time::Duration;
use widestring::U16CString;
use windows_service::service::{ServiceAccess, ServiceState};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{FlushFileBuffers, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

pub const SERVICE_NAME: &str = "LogCrateIndex";
pub const PIPE_NAME: &str = r"\\.\pipe\LogCrate.Index.v2";
pub const PROTOCOL_VERSION: u16 = 2;
const MAGIC: [u8; 4] = *b"LCIX";
const HEADER_SIZE: usize = 12;
const MAX_FRAME_BODY: usize = 8 * 1024 * 1024;
const MAX_BATCH_RECORDS: usize = 131_072;
const MAX_CONCURRENT_CLIENTS: usize = 4;
// CreateNamedPipeW only uses this value as the namespace-wide instance ceiling; the
// server still creates a single listener at a time and bounds active work separately.
// Keeping the transport ceiling above the worker budget lets an extra client connect
// long enough to receive the stable retryable busy response instead of killing accept.
const MAX_PIPE_INSTANCES: u32 = 255;
const BUSY_RESPONSE_CODE: u32 = 429;
const PIPE_BUFFER_SIZE: u32 = 1024 * 1024;
const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_BROKEN_PIPE: i32 = 109;
const ERROR_SEM_TIMEOUT: i32 = 121;
const ERROR_PIPE_BUSY: i32 = 231;
const ERROR_NO_DATA: i32 = 232;
const ERROR_PIPE_NOT_CONNECTED: i32 = 233;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_CANCELLED: i32 = 1223;
const CLIENT_RETRY_ATTEMPTS: usize = 8;
const CLIENT_RETRY_INITIAL_DELAY: Duration = Duration::from_millis(25);
const CLIENT_RETRY_MAX_DELAY: Duration = Duration::from_millis(1_000);
const REPAIR_EXECUTABLE_NAME: &str = "logcrate_index_service.exe";
const REPAIR_ARGUMENTS: &str = "--install";

const REQUEST_HELLO: u16 = 1;
const REQUEST_ENUMERATE_MFT: u16 = 2;
const REQUEST_QUERY_USN: u16 = 3;
const REQUEST_READ_USN: u16 = 4;
const RESPONSE_HELLO: u16 = 100;
const RESPONSE_MFT_BATCH: u16 = 101;
const RESPONSE_COMPLETE: u16 = 102;
const RESPONSE_ERROR: u16 = 103;
const RESPONSE_USN_INFO: u16 = 104;
const RESPONSE_USN_BATCH: u16 = 105;
const RESPONSE_USN_COMPLETE: u16 = 106;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Hello,
    EnumerateMft {
        volume: char,
    },
    QueryUsn {
        volume: char,
    },
    ReadUsn {
        volume: char,
        start_usn: i64,
        journal_id: u64,
        target_usn: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Hello { protocol: u16 },
    MftBatch(Vec<MftRecord>),
    Complete(MftEnumeration),
    UsnInfo(UsnJournalInfo),
    UsnBatch(Vec<MftRecord>),
    UsnComplete(UsnReadSummary),
    Error { code: u32, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceFailureCode {
    Missing,
    Busy,
    PipeMissing,
    Starting,
    Stopped,
    AccessDenied,
    StartFailed,
    NotReady,
    ProtocolMismatch,
    ElevationCancelled,
    RepairExecutableMissing,
    RepairFailed,
}

impl ServiceFailureCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Busy => "busy",
            Self::PipeMissing => "pipeMissing",
            Self::Starting => "starting",
            Self::Stopped => "stopped",
            Self::AccessDenied => "accessDenied",
            Self::StartFailed => "startFailed",
            Self::NotReady => "notReady",
            Self::ProtocolMismatch => "protocolMismatch",
            Self::ElevationCancelled => "elevationCancelled",
            Self::RepairExecutableMissing => "repairExecutableMissing",
            Self::RepairFailed => "repairFailed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientRecoveryClass {
    RetryWithinRound,
    RetryNextRound,
}

impl ServiceFailureCode {
    const fn recovery_class(self) -> ClientRecoveryClass {
        match self {
            Self::Busy | Self::PipeMissing | Self::Starting => {
                ClientRecoveryClass::RetryWithinRound
            }
            Self::Missing
            | Self::Stopped
            | Self::AccessDenied
            | Self::StartFailed
            | Self::NotReady
            | Self::ProtocolMismatch
            | Self::ElevationCancelled
            | Self::RepairExecutableMissing
            | Self::RepairFailed => ClientRecoveryClass::RetryNextRound,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("[{code}] {message}", code = .code.as_str())]
pub struct ServiceFailure {
    pub code: ServiceFailureCode,
    pub message: String,
}

impl ServiceFailure {
    fn new(code: ServiceFailureCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("IPC 协议版本不兼容: {0}")]
struct ProtocolVersionMismatch(u16);

pub fn enumerate_mft_via_service<F>(volume: char, mut on_batch: F) -> anyhow::Result<MftEnumeration>
where
    F: FnMut(Vec<MftRecord>) -> anyhow::Result<()>,
{
    let mut pipe = connect_and_handshake()?;
    write_request(&mut pipe, &Request::EnumerateMft { volume })?;
    loop {
        match read_response(&mut pipe)? {
            Response::MftBatch(records) => on_batch(records)?,
            Response::Complete(summary) => return Ok(summary),
            Response::Error { code, message } => bail!("索引服务错误 {code}: {message}"),
            response => bail!("索引服务枚举响应无效: {response:?}"),
        }
    }
}

pub fn query_usn_via_service(volume: char) -> anyhow::Result<UsnJournalInfo> {
    let mut pipe = connect_and_handshake()?;
    write_request(&mut pipe, &Request::QueryUsn { volume })?;
    match read_response(&mut pipe)? {
        Response::UsnInfo(info) => Ok(info),
        Response::Error { code, message } => bail!("索引服务错误 {code}: {message}"),
        response => bail!("索引服务 USN 信息响应无效: {response:?}"),
    }
}

pub fn read_usn_via_service<F>(
    volume: char,
    start_usn: i64,
    journal_id: u64,
    target_usn: i64,
    mut on_batch: F,
) -> anyhow::Result<UsnReadSummary>
where
    F: FnMut(Vec<MftRecord>) -> anyhow::Result<()>,
{
    let mut pipe = connect_and_handshake()?;
    write_request(
        &mut pipe,
        &Request::ReadUsn {
            volume,
            start_usn,
            journal_id,
            target_usn,
        },
    )?;
    loop {
        match read_response(&mut pipe)? {
            Response::UsnBatch(records) => on_batch(records)?,
            Response::UsnComplete(summary) => return Ok(summary),
            Response::Error { code, message } => bail!("索引服务错误 {code}: {message}"),
            response => bail!("索引服务 USN 读取响应无效: {response:?}"),
        }
    }
}

pub fn repair_service() -> Result<(), ServiceFailure> {
    let executable = repair_executable_path(&std::env::current_exe().map_err(|error| {
        ServiceFailure::new(
            ServiceFailureCode::RepairFailed,
            format!("无法确定 LogCrate 安装位置: {error}"),
        )
    })?)?;
    validate_repair_executable(&executable)?;
    let exit_code = run_elevated_repair(&executable)?;
    interpret_repair_exit_code(exit_code)?;
    start_installed_service()?;
    connect_and_handshake_diagnostic().map(|_| ())
}

fn connect_and_handshake() -> anyhow::Result<File> {
    connect_and_handshake_diagnostic().map_err(anyhow::Error::new)
}

fn connect_and_handshake_diagnostic() -> Result<File, ServiceFailure> {
    run_client_recovery_round(connect_and_handshake_once, sleep)
}

fn run_client_recovery_round<T, Attempt, Wait>(
    mut attempt_connection: Attempt,
    mut wait: Wait,
) -> Result<T, ServiceFailure>
where
    Attempt: FnMut() -> Result<T, ServiceFailure>,
    Wait: FnMut(Duration),
{
    let mut last_failure = None;
    for attempt in 0..CLIENT_RETRY_ATTEMPTS {
        match attempt_connection() {
            Ok(value) => return Ok(value),
            Err(failure)
                if failure.code.recovery_class() == ClientRecoveryClass::RetryWithinRound =>
            {
                last_failure = Some(failure);
                if let Some(delay) = client_retry_delay(attempt) {
                    wait(delay);
                }
            }
            Err(failure) => return Err(failure),
        }
    }
    Err(last_failure.unwrap_or_else(|| {
        ServiceFailure::new(
            ServiceFailureCode::NotReady,
            "索引服务客户端恢复轮未完成连接",
        )
    }))
}

fn connect_and_handshake_once() -> Result<File, ServiceFailure> {
    let mut pipe = connect()?;
    write_request(&mut pipe, &Request::Hello).map_err(handshake_failure)?;
    match read_response(&mut pipe).map_err(handshake_failure)? {
        Response::Hello { protocol } if protocol == PROTOCOL_VERSION => Ok(pipe),
        Response::Hello { protocol } => Err(ServiceFailure::new(
            ServiceFailureCode::ProtocolMismatch,
            format!("索引服务协议版本不兼容: {protocol}"),
        )),
        Response::Error { code, message } if code == BUSY_RESPONSE_CODE => Err(
            ServiceFailure::new(ServiceFailureCode::Busy, format!("索引服务繁忙: {message}")),
        ),
        Response::Error { code, message } => Err(ServiceFailure::new(
            ServiceFailureCode::NotReady,
            format!("索引服务握手失败 {code}: {message}"),
        )),
        response => Err(ServiceFailure::new(
            ServiceFailureCode::ProtocolMismatch,
            format!("索引服务握手响应不兼容: {response:?}"),
        )),
    }
}

pub fn run_pipe_server(stop: &AtomicBool, once: bool) -> anyhow::Result<()> {
    run_pipe_server_at(PIPE_NAME, stop, once)
}

fn run_pipe_server_at(pipe_name: &str, stop: &AtomicBool, once: bool) -> anyhow::Result<()> {
    if once {
        let pipe = create_server_pipe(pipe_name)?;
        connect_server_pipe(&pipe)?;
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        return serve_connected_pipe(pipe);
    }

    let active = AtomicUsize::new(0);
    thread::scope(|scope| -> anyhow::Result<()> {
        loop {
            let pipe = create_server_pipe(pipe_name)?;
            connect_server_pipe(&pipe)?;
            if stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let Some(guard) = try_acquire_client_slot(&active) else {
                reject_busy_client(pipe);
                continue;
            };
            scope.spawn(move || {
                let _guard = guard;
                let _ = serve_connected_pipe(pipe);
            });
        }
    })
}

fn connect_server_pipe(pipe: &OwnedPipe) -> anyhow::Result<()> {
    let connected = unsafe { ConnectNamedPipe(pipe.0, null_mut()) };
    if connected == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(535) {
            return Err(error).context("等待索引服务 named pipe 客户端失败");
        }
    }
    Ok(())
}

fn serve_connected_pipe(pipe: OwnedPipe) -> anyhow::Result<()> {
    let raw = pipe.0 as RawHandle;
    std::mem::forget(pipe);
    let mut file = unsafe { File::from_raw_handle(raw) };
    let served = serve_client(&mut file);
    let _ = file.flush();
    unsafe {
        DisconnectNamedPipe(raw as HANDLE);
    }
    drop(file);
    served
}

fn reject_busy_client(pipe: OwnedPipe) {
    let raw = pipe.0 as RawHandle;
    std::mem::forget(pipe);
    let mut file = unsafe { File::from_raw_handle(raw) };
    let _ = write_response(
        &mut file,
        &Response::Error {
            code: BUSY_RESPONSE_CODE,
            message: "索引服务并发请求已达上限".into(),
        },
    );
    unsafe {
        FlushFileBuffers(raw as HANDLE);
        DisconnectNamedPipe(raw as HANDLE);
    }
}

struct ActiveClientGuard<'a>(&'a AtomicUsize);

fn try_acquire_client_slot(active: &AtomicUsize) -> Option<ActiveClientGuard<'_>> {
    let mut current = active.load(Ordering::Acquire);
    loop {
        if current >= MAX_CONCURRENT_CLIENTS {
            return None;
        }
        match active.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Some(ActiveClientGuard(active)),
            Err(next) => current = next,
        }
    }
}

impl Drop for ActiveClientGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn wake_pipe_server() {
    wake_pipe_server_at(PIPE_NAME);
}

fn wake_pipe_server_at(pipe_name: &str) {
    let _ = OpenOptions::new().read(true).write(true).open(pipe_name);
}

fn serve_client(pipe: &mut File) -> anyhow::Result<()> {
    loop {
        let request = match read_request(pipe) {
            Ok(request) => request,
            Err(error) if is_disconnect(&error) => return Ok(()),
            Err(error) => return Err(error),
        };
        match request {
            Request::Hello => write_response(
                pipe,
                &Response::Hello {
                    protocol: PROTOCOL_VERSION,
                },
            )?,
            Request::EnumerateMft { volume } => {
                // A disconnected client makes the batch write fail. Propagating that failure from
                // the enumeration callback is the cancellation signal for the current FSCTL loop.
                let result = super::enumerate_mft(volume, |records| {
                    write_response(pipe, &Response::MftBatch(records))
                });
                match result {
                    Ok(summary) => write_response(pipe, &Response::Complete(summary))?,
                    Err(error) => {
                        let code = error
                            .chain()
                            .find_map(|cause| cause.downcast_ref::<io::Error>())
                            .and_then(io::Error::raw_os_error)
                            .unwrap_or(1) as u32;
                        write_response(
                            pipe,
                            &Response::Error {
                                code,
                                message: format!("{error:#}"),
                            },
                        )?;
                    }
                }
            }
            Request::QueryUsn { volume } => match super::query_usn_journal(volume) {
                Ok(info) => write_response(pipe, &Response::UsnInfo(info))?,
                Err(error) => write_service_error(pipe, &error)?,
            },
            Request::ReadUsn {
                volume,
                start_usn,
                journal_id,
                target_usn,
            } => {
                let result =
                    super::read_usn_journal(volume, start_usn, journal_id, target_usn, |records| {
                        write_response(pipe, &Response::UsnBatch(records))
                    });
                match result {
                    Ok(summary) => write_response(pipe, &Response::UsnComplete(summary))?,
                    Err(error) => write_service_error(pipe, &error)?,
                }
            }
        }
    }
}

fn write_service_error(pipe: &mut File, error: &anyhow::Error) -> anyhow::Result<()> {
    let code = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<io::Error>())
        .and_then(io::Error::raw_os_error)
        .unwrap_or(1) as u32;
    write_response(
        pipe,
        &Response::Error {
            code,
            message: format!("{error:#}"),
        },
    )
}

fn connect() -> Result<File, ServiceFailure> {
    match open_pipe() {
        Ok(pipe) => return Ok(pipe),
        Err(error) => match pipe_open_failure(error) {
            failure if failure.code == ServiceFailureCode::PipeMissing => {}
            failure => return Err(failure),
        },
    }
    match start_installed_service()? {
        ServiceState::Running => Err(ServiceFailure::new(
            ServiceFailureCode::PipeMissing,
            "LogCrate Index Service 正在运行，但 named pipe 尚不存在",
        )),
        ServiceState::StartPending | ServiceState::ContinuePending => Err(ServiceFailure::new(
            ServiceFailureCode::Starting,
            "LogCrate Index Service 正在启动",
        )),
        state => Err(service_state_failure(state)),
    }
}

fn open_pipe() -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(PIPE_NAME)
}

fn pipe_open_failure(error: io::Error) -> ServiceFailure {
    let code = match error.raw_os_error() {
        Some(ERROR_ACCESS_DENIED) => ServiceFailureCode::AccessDenied,
        Some(ERROR_PIPE_BUSY | ERROR_SEM_TIMEOUT) => ServiceFailureCode::Busy,
        Some(
            ERROR_FILE_NOT_FOUND | ERROR_BROKEN_PIPE | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED,
        ) => ServiceFailureCode::PipeMissing,
        _ => ServiceFailureCode::NotReady,
    };
    ServiceFailure::new(
        code,
        format!("无法连接 LogCrate Index Service named pipe: {error}"),
    )
}

fn start_installed_service() -> Result<ServiceState, ServiceFailure> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|error| classify_service_error("连接 Windows Service Control Manager", error))?;
    let service = manager
        .open_service(
            SERVICE_NAME,
            ServiceAccess::START | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|error| classify_service_error("打开 LogCrate Index Service", error))?;
    let state = service
        .query_status()
        .map_err(|error| classify_service_error("查询 LogCrate Index Service 状态", error))?
        .current_state;
    match state {
        ServiceState::Running | ServiceState::StartPending | ServiceState::ContinuePending => {
            Ok(state)
        }
        ServiceState::Stopped => {
            service
                .start::<&str>(&[])
                .map_err(|error| classify_service_error("启动 LogCrate Index Service", error))?;
            Ok(ServiceState::StartPending)
        }
        state => Err(service_state_failure(state)),
    }
}

fn service_state_failure(state: ServiceState) -> ServiceFailure {
    let code = if matches!(state, ServiceState::StopPending | ServiceState::Stopped) {
        ServiceFailureCode::Stopped
    } else {
        ServiceFailureCode::StartFailed
    };
    ServiceFailure::new(
        code,
        format!("LogCrate Index Service 状态不可用: {state:?}"),
    )
}

fn client_retry_delay(attempt: usize) -> Option<Duration> {
    if attempt.saturating_add(1) >= CLIENT_RETRY_ATTEMPTS {
        return None;
    }
    let multiplier = 1_u32
        .checked_shl(attempt.min(31) as u32)
        .unwrap_or(u32::MAX);
    Some(
        CLIENT_RETRY_INITIAL_DELAY
            .saturating_mul(multiplier)
            .min(CLIENT_RETRY_MAX_DELAY),
    )
}

fn classify_service_error(stage: &str, error: windows_service::Error) -> ServiceFailure {
    let raw_code = match &error {
        windows_service::Error::Winapi(error) => error.raw_os_error(),
        _ => None,
    };
    let code = classify_service_win32_code(raw_code);
    let detail = raw_code
        .map(|raw| format!("Win32 {raw}"))
        .unwrap_or_else(|| error.to_string());
    ServiceFailure::new(code, format!("{stage}失败（{detail}）"))
}

fn classify_service_win32_code(raw_code: Option<i32>) -> ServiceFailureCode {
    match raw_code {
        Some(ERROR_SERVICE_DOES_NOT_EXIST) => ServiceFailureCode::Missing,
        Some(ERROR_ACCESS_DENIED) => ServiceFailureCode::AccessDenied,
        _ => ServiceFailureCode::StartFailed,
    }
}

fn handshake_failure(error: anyhow::Error) -> ServiceFailure {
    if let Some(mismatch) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ProtocolVersionMismatch>())
    {
        return ServiceFailure::new(ServiceFailureCode::ProtocolMismatch, mismatch.to_string());
    }
    if let Some(io_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<io::Error>())
    {
        let failure = pipe_open_failure(io::Error::from_raw_os_error(
            io_error.raw_os_error().unwrap_or(1),
        ));
        return ServiceFailure::new(failure.code, format!("索引服务 IPC 握手失败: {error:#}"));
    }
    ServiceFailure::new(
        ServiceFailureCode::NotReady,
        format!("索引服务 IPC 握手未就绪: {error:#}"),
    )
}

fn repair_executable_path(gui_executable: &Path) -> Result<PathBuf, ServiceFailure> {
    let directory = gui_executable.parent().ok_or_else(|| {
        ServiceFailure::new(
            ServiceFailureCode::RepairExecutableMissing,
            "LogCrate 可执行文件没有可用的安装目录",
        )
    })?;
    Ok(directory.join(REPAIR_EXECUTABLE_NAME))
}

fn validate_repair_executable(executable: &Path) -> Result<(), ServiceFailure> {
    if executable.is_file() {
        return Ok(());
    }
    Err(ServiceFailure::new(
        ServiceFailureCode::RepairExecutableMissing,
        format!("修复程序不存在: {}", executable.display()),
    ))
}

fn interpret_repair_exit_code(exit_code: u32) -> Result<(), ServiceFailure> {
    if exit_code == 0 {
        return Ok(());
    }
    Err(ServiceFailure::new(
        ServiceFailureCode::RepairFailed,
        format!("索引服务重新注册程序退出码为 {exit_code}"),
    ))
}

fn run_elevated_repair(executable: &Path) -> Result<u32, ServiceFailure> {
    let verb = U16CString::from_str("runas").expect("runas does not contain NUL");
    let parameters =
        U16CString::from_str(REPAIR_ARGUMENTS).expect("fixed repair arguments do not contain NUL");
    let executable_wide = U16CString::from_os_str(executable.as_os_str()).map_err(|error| {
        ServiceFailure::new(
            ServiceFailureCode::RepairFailed,
            format!("修复程序路径无效: {error}"),
        )
    })?;
    let directory_wide = executable
        .parent()
        .map(|directory| U16CString::from_os_str(directory.as_os_str()))
        .transpose()
        .map_err(|error| {
            ServiceFailure::new(
                ServiceFailureCode::RepairFailed,
                format!("修复程序工作目录无效: {error}"),
            )
        })?;
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: executable_wide.as_ptr(),
        lpParameters: parameters.as_ptr(),
        lpDirectory: directory_wide
            .as_ref()
            .map_or(std::ptr::null(), |directory| directory.as_ptr()),
        nShow: 1,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut execute) } == 0 {
        let error = io::Error::last_os_error();
        return Err(classify_elevation_launch_error(error));
    }
    if execute.hProcess.is_null() {
        return Err(ServiceFailure::new(
            ServiceFailureCode::RepairFailed,
            "提升权限的索引服务修复程序未返回进程句柄",
        ));
    }
    let process = OwnedHandle(execute.hProcess);
    let wait = unsafe { WaitForSingleObject(process.0, INFINITE) };
    if wait == WAIT_FAILED {
        return Err(ServiceFailure::new(
            ServiceFailureCode::RepairFailed,
            format!(
                "等待提升权限的索引服务修复程序失败: {}",
                io::Error::last_os_error()
            ),
        ));
    }
    if wait != WAIT_OBJECT_0 {
        return Err(ServiceFailure::new(
            ServiceFailureCode::RepairFailed,
            format!("等待提升权限的索引服务修复程序返回异常状态 {wait}"),
        ));
    }
    let mut exit_code = 0;
    if unsafe { GetExitCodeProcess(process.0, &mut exit_code) } == 0 {
        return Err(ServiceFailure::new(
            ServiceFailureCode::RepairFailed,
            format!(
                "读取索引服务修复程序退出码失败: {}",
                io::Error::last_os_error()
            ),
        ));
    }
    Ok(exit_code)
}

fn classify_elevation_launch_error(error: io::Error) -> ServiceFailure {
    if error.raw_os_error() == Some(ERROR_CANCELLED) {
        ServiceFailure::new(
            ServiceFailureCode::ElevationCancelled,
            "用户取消了索引服务修复授权",
        )
    } else {
        ServiceFailure::new(
            ServiceFailureCode::RepairFailed,
            format!("无法启动提升权限的索引服务修复程序: {error}"),
        )
    }
}

fn write_request(writer: &mut impl Write, request: &Request) -> anyhow::Result<()> {
    let (kind, body) = encode_request(request)?;
    write_frame(writer, kind, &body)
}

fn read_request(reader: &mut impl Read) -> anyhow::Result<Request> {
    let (kind, body) = read_frame(reader)?;
    decode_request(kind, &body)
}

fn write_response(writer: &mut impl Write, response: &Response) -> anyhow::Result<()> {
    let (kind, body) = encode_response(response)?;
    write_frame(writer, kind, &body)
}

fn read_response(reader: &mut impl Read) -> anyhow::Result<Response> {
    let (kind, body) = read_frame(reader)?;
    decode_response(kind, &body)
}

fn write_frame(writer: &mut impl Write, kind: u16, body: &[u8]) -> anyhow::Result<()> {
    if body.len() > MAX_FRAME_BODY {
        bail!("IPC 帧超过大小上限");
    }
    let mut header = [0_u8; HEADER_SIZE];
    header[..4].copy_from_slice(&MAGIC);
    header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    header[6..8].copy_from_slice(&kind.to_le_bytes());
    header[8..12].copy_from_slice(&(body.len() as u32).to_le_bytes());
    writer.write_all(&header)?;
    writer.write_all(body)?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl Read) -> anyhow::Result<(u16, Vec<u8>)> {
    let mut header = [0_u8; HEADER_SIZE];
    reader.read_exact(&mut header)?;
    if header[..4] != MAGIC {
        bail!("IPC magic 无效");
    }
    let protocol = u16::from_le_bytes([header[4], header[5]]);
    if protocol != PROTOCOL_VERSION {
        return Err(ProtocolVersionMismatch(protocol).into());
    }
    let kind = u16::from_le_bytes([header[6], header[7]]);
    let body_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    if body_len > MAX_FRAME_BODY {
        bail!("IPC 帧声明长度超过上限");
    }
    let mut body = vec![0_u8; body_len];
    reader.read_exact(&mut body)?;
    Ok((kind, body))
}

fn encode_request(request: &Request) -> anyhow::Result<(u16, Vec<u8>)> {
    match request {
        Request::Hello => Ok((REQUEST_HELLO, Vec::new())),
        Request::EnumerateMft { volume } if volume.is_ascii_alphabetic() => Ok((
            REQUEST_ENUMERATE_MFT,
            vec![volume.to_ascii_uppercase() as u8],
        )),
        Request::QueryUsn { volume } if volume.is_ascii_alphabetic() => {
            Ok((REQUEST_QUERY_USN, vec![volume.to_ascii_uppercase() as u8]))
        }
        Request::ReadUsn {
            volume,
            start_usn,
            journal_id,
            target_usn,
        } if volume.is_ascii_alphabetic() && start_usn <= target_usn => {
            let mut body = Vec::with_capacity(25);
            body.push(volume.to_ascii_uppercase() as u8);
            body.extend(start_usn.to_le_bytes());
            body.extend(journal_id.to_le_bytes());
            body.extend(target_usn.to_le_bytes());
            Ok((REQUEST_READ_USN, body))
        }
        Request::EnumerateMft { .. } | Request::QueryUsn { .. } | Request::ReadUsn { .. } => {
            bail!("USN 请求的卷或范围无效")
        }
    }
}

fn decode_request(kind: u16, body: &[u8]) -> anyhow::Result<Request> {
    match (kind, body) {
        (REQUEST_HELLO, []) => Ok(Request::Hello),
        (REQUEST_ENUMERATE_MFT, [volume]) if (*volume as char).is_ascii_alphabetic() => {
            Ok(Request::EnumerateMft {
                volume: (*volume as char).to_ascii_uppercase(),
            })
        }
        (REQUEST_QUERY_USN, [volume]) if (*volume as char).is_ascii_alphabetic() => {
            Ok(Request::QueryUsn {
                volume: (*volume as char).to_ascii_uppercase(),
            })
        }
        (REQUEST_READ_USN, body) if body.len() == 25 && (body[0] as char).is_ascii_alphabetic() => {
            let start_usn = i64::from_le_bytes(read_array(body, 1)?);
            let target_usn = i64::from_le_bytes(read_array(body, 17)?);
            if start_usn > target_usn {
                bail!("USN 读取范围无效");
            }
            Ok(Request::ReadUsn {
                volume: (body[0] as char).to_ascii_uppercase(),
                start_usn,
                journal_id: read_u64(body, 9)?,
                target_usn,
            })
        }
        _ => bail!("IPC 请求类型或长度无效"),
    }
}

fn encode_response(response: &Response) -> anyhow::Result<(u16, Vec<u8>)> {
    match response {
        Response::Hello { protocol } => Ok((RESPONSE_HELLO, protocol.to_le_bytes().to_vec())),
        Response::MftBatch(records) => Ok((RESPONSE_MFT_BATCH, encode_records(records)?)),
        Response::Complete(summary) => {
            let mut body = Vec::with_capacity(24);
            body.extend(summary.batches.to_le_bytes());
            body.extend(summary.records.to_le_bytes());
            body.extend(summary.last_reference.to_le_bytes());
            Ok((RESPONSE_COMPLETE, body))
        }
        Response::UsnInfo(info) => {
            let mut body = Vec::with_capacity(32);
            body.extend(info.journal_id.to_le_bytes());
            body.extend(info.first_usn.to_le_bytes());
            body.extend(info.next_usn.to_le_bytes());
            body.extend(info.lowest_valid_usn.to_le_bytes());
            Ok((RESPONSE_USN_INFO, body))
        }
        Response::UsnBatch(records) => Ok((RESPONSE_USN_BATCH, encode_records(records)?)),
        Response::UsnComplete(summary) => {
            let mut body = Vec::with_capacity(24);
            body.extend(summary.batches.to_le_bytes());
            body.extend(summary.records.to_le_bytes());
            body.extend(summary.next_usn.to_le_bytes());
            Ok((RESPONSE_USN_COMPLETE, body))
        }
        Response::Error { code, message } => {
            let message = message.as_bytes();
            let mut body = Vec::with_capacity(8 + message.len());
            body.extend(code.to_le_bytes());
            body.extend((message.len() as u32).to_le_bytes());
            body.extend(message);
            Ok((RESPONSE_ERROR, body))
        }
    }
}

fn decode_response(kind: u16, body: &[u8]) -> anyhow::Result<Response> {
    match kind {
        RESPONSE_HELLO if body.len() == 2 => Ok(Response::Hello {
            protocol: u16::from_le_bytes(body.try_into().unwrap()),
        }),
        RESPONSE_MFT_BATCH => Ok(Response::MftBatch(decode_records(body)?)),
        RESPONSE_COMPLETE if body.len() == 24 => Ok(Response::Complete(MftEnumeration {
            batches: read_u64(body, 0)?,
            records: read_u64(body, 8)?,
            last_reference: read_u64(body, 16)?,
        })),
        RESPONSE_USN_INFO if body.len() == 32 => Ok(Response::UsnInfo(UsnJournalInfo {
            journal_id: read_u64(body, 0)?,
            first_usn: i64::from_le_bytes(read_array(body, 8)?),
            next_usn: i64::from_le_bytes(read_array(body, 16)?),
            lowest_valid_usn: i64::from_le_bytes(read_array(body, 24)?),
        })),
        RESPONSE_USN_BATCH => Ok(Response::UsnBatch(decode_records(body)?)),
        RESPONSE_USN_COMPLETE if body.len() == 24 => Ok(Response::UsnComplete(UsnReadSummary {
            batches: read_u64(body, 0)?,
            records: read_u64(body, 8)?,
            next_usn: i64::from_le_bytes(read_array(body, 16)?),
        })),
        RESPONSE_ERROR if body.len() >= 8 => {
            let code = read_u32(body, 0)?;
            let length = read_u32(body, 4)? as usize;
            if length > MAX_FRAME_BODY || body.len() != 8 + length {
                bail!("IPC 错误消息长度无效");
            }
            Ok(Response::Error {
                code,
                message: String::from_utf8(body[8..].to_vec()).context("IPC 错误消息不是 UTF-8")?,
            })
        }
        _ => bail!("IPC 响应类型或长度无效"),
    }
}

fn encode_records(records: &[MftRecord]) -> anyhow::Result<Vec<u8>> {
    if records.len() > MAX_BATCH_RECORDS {
        bail!("MFT IPC 批次记录数超过上限");
    }
    let mut body = Vec::new();
    body.extend((records.len() as u32).to_le_bytes());
    for record in records {
        let name = record.name.as_bytes();
        if name.len() > u32::MAX as usize || name.len() > MAX_FRAME_BODY {
            bail!("MFT 文件名超过 IPC 上限");
        }
        body.extend(record.id.as_bytes());
        body.extend(record.parent_id.as_bytes());
        body.extend(record.usn.to_le_bytes());
        body.extend(record.attributes.to_le_bytes());
        body.extend(record.reason.to_le_bytes());
        body.extend((name.len() as u32).to_le_bytes());
        body.extend(name);
        if body.len() > MAX_FRAME_BODY {
            bail!("MFT IPC 批次超过帧大小上限");
        }
    }
    Ok(body)
}

fn decode_records(body: &[u8]) -> anyhow::Result<Vec<MftRecord>> {
    if body.len() < 4 {
        bail!("MFT IPC 批次缺少记录数");
    }
    let count = read_u32(body, 0)? as usize;
    if count > MAX_BATCH_RECORDS {
        bail!("MFT IPC 批次记录数超过上限");
    }
    let mut offset = 4;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let id = super::FileId::from_bytes(read_array(body, offset)?);
        offset += 16;
        let parent_id = super::FileId::from_bytes(read_array(body, offset)?);
        offset += 16;
        let usn = i64::from_le_bytes(read_array(body, offset)?);
        offset += 8;
        let attributes = read_u32(body, offset)?;
        offset += 4;
        let reason = read_u32(body, offset)?;
        offset += 4;
        let name_len = read_u32(body, offset)? as usize;
        offset += 4;
        let end = offset
            .checked_add(name_len)
            .filter(|end| *end <= body.len())
            .ok_or_else(|| anyhow!("MFT IPC 文件名越过批次边界"))?;
        let name =
            String::from_utf8(body[offset..end].to_vec()).context("MFT IPC 文件名不是 UTF-8")?;
        offset = end;
        records.push(MftRecord {
            id,
            parent_id,
            name,
            attributes,
            reason,
            usn,
        });
    }
    if offset != body.len() {
        bail!("MFT IPC 批次包含尾随数据");
    }
    Ok(records)
}

struct OwnedPipe(HANDLE);

// Windows kernel handles are process-wide. Ownership is moved to exactly one scoped client
// thread, which closes the handle through File/Drop before that thread exits.
unsafe impl Send for OwnedPipe {}

impl Drop for OwnedPipe {
    fn drop(&mut self) {
        if self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct OwnedSecurityDescriptor(*mut c_void);

impl Drop for OwnedSecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0 as HLOCAL);
            }
        }
    }
}

fn create_server_pipe(pipe_name: &str) -> anyhow::Result<OwnedPipe> {
    let sddl = U16CString::from_str(PIPE_SDDL)?;
    let mut descriptor = null_mut::<c_void>();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error()).context("创建索引服务 pipe ACL 失败");
    }
    let descriptor = OwnedSecurityDescriptor(descriptor);
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let name = U16CString::from_str(pipe_name)?;
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            MAX_PIPE_INSTANCES,
            PIPE_BUFFER_SIZE,
            PIPE_BUFFER_SIZE,
            0,
            &security,
        )
    };
    drop(descriptor);
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error()).context("创建索引服务 named pipe 失败");
    }
    Ok(OwnedPipe(handle))
}

fn read_u32(bytes: &[u8], offset: usize) -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_u64(bytes: &[u8], offset: usize) -> anyhow::Result<u64> {
    Ok(u64::from_le_bytes(read_array(bytes, offset)?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> anyhow::Result<[u8; N]> {
    bytes
        .get(offset..offset + N)
        .ok_or_else(|| anyhow!("IPC 字段越过边界"))?
        .try_into()
        .map_err(|_| anyhow!("IPC 字段长度无效"))
}

fn is_disconnect(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .and_then(io::Error::raw_os_error)
            .is_some_and(|code| matches!(code, 109 | 232 | 233))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ntfs::FileId;
    use std::io::Cursor;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::sync::{mpsc, Arc};
    use std::time::Instant;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    static TEST_PIPE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    fn unique_test_pipe_name() -> String {
        let sequence = TEST_PIPE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        format!(
            r"\\.\pipe\LogCrate.Index.Test.{}.{}",
            std::process::id(),
            sequence
        )
    }

    fn open_test_pipe(pipe_name: &str, deadline: Instant) -> File {
        loop {
            match OpenOptions::new().read(true).write(true).open(pipe_name) {
                Ok(pipe) => return pipe,
                Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(10)),
                Err(error) => panic!("连接测试 named pipe {pipe_name} 失败: {error}"),
            }
        }
    }

    fn read_test_response(pipe: &mut File, deadline: Instant, stage: &str) -> Response {
        let mut header = [0_u8; HEADER_SIZE];
        loop {
            let mut header_bytes = 0_u32;
            let mut available = 0_u32;
            let peeked = unsafe {
                PeekNamedPipe(
                    pipe.as_raw_handle() as HANDLE,
                    header.as_mut_ptr().cast(),
                    HEADER_SIZE as u32,
                    &mut header_bytes,
                    &mut available,
                    null_mut(),
                )
            };
            let peek_error = (peeked == 0)
                .then(|| io::Error::last_os_error().raw_os_error())
                .flatten();
            if peeked != 0 && header_bytes as usize == HEADER_SIZE {
                let body_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
                if available as usize >= HEADER_SIZE + body_len {
                    return read_response(pipe).unwrap();
                }
            }
            assert!(
                Instant::now() < deadline,
                "等待测试 named pipe {stage} 响应超时: error={peek_error:?}, header_bytes={header_bytes}, available={available}"
            );
            sleep(Duration::from_millis(10));
        }
    }

    fn connect_test_client(pipe_name: &str, deadline: Instant) -> File {
        loop {
            let mut pipe = open_test_pipe(pipe_name, deadline);
            write_request(&mut pipe, &Request::Hello).unwrap();
            match read_test_response(&mut pipe, deadline, "handshake") {
                Response::Hello { protocol } if protocol == PROTOCOL_VERSION => return pipe,
                Response::Error {
                    code: BUSY_RESPONSE_CODE,
                    ..
                } if Instant::now() < deadline => sleep(Duration::from_millis(10)),
                response => panic!("测试 named pipe 握手响应无效: {response:?}"),
            }
        }
    }

    fn record() -> MftRecord {
        MftRecord {
            id: FileId::from_u64(10),
            parent_id: FileId::from_u64(5),
            name: "调试-debug.log".into(),
            attributes: 32,
            reason: 0,
            usn: 99,
        }
    }

    #[test]
    fn service_failure_codes_are_stable_and_stage_specific() {
        assert_eq!(
            classify_service_win32_code(Some(ERROR_SERVICE_DOES_NOT_EXIST)),
            ServiceFailureCode::Missing
        );
        assert_eq!(
            classify_service_win32_code(Some(ERROR_ACCESS_DENIED)),
            ServiceFailureCode::AccessDenied
        );
        assert_eq!(
            classify_service_win32_code(Some(1053)),
            ServiceFailureCode::StartFailed
        );
        assert_eq!(ServiceFailureCode::Missing.as_str(), "missing");
        assert_eq!(ServiceFailureCode::Busy.as_str(), "busy");
        assert_eq!(ServiceFailureCode::PipeMissing.as_str(), "pipeMissing");
        assert_eq!(ServiceFailureCode::Starting.as_str(), "starting");
        assert_eq!(ServiceFailureCode::Stopped.as_str(), "stopped");
        assert_eq!(ServiceFailureCode::AccessDenied.as_str(), "accessDenied");
        assert_eq!(ServiceFailureCode::StartFailed.as_str(), "startFailed");
        assert_eq!(ServiceFailureCode::NotReady.as_str(), "notReady");
        assert_eq!(
            ServiceFailureCode::ProtocolMismatch.as_str(),
            "protocolMismatch"
        );
        assert_eq!(
            ServiceFailureCode::ElevationCancelled.as_str(),
            "elevationCancelled"
        );
        assert_eq!(
            ServiceFailureCode::RepairExecutableMissing.as_str(),
            "repairExecutableMissing"
        );
        assert_eq!(ServiceFailureCode::RepairFailed.as_str(), "repairFailed");

        assert_eq!(
            ServiceFailureCode::Busy.recovery_class(),
            ClientRecoveryClass::RetryWithinRound
        );
        assert_eq!(
            ServiceFailureCode::PipeMissing.recovery_class(),
            ClientRecoveryClass::RetryWithinRound
        );
        assert_eq!(
            ServiceFailureCode::Starting.recovery_class(),
            ClientRecoveryClass::RetryWithinRound
        );
        assert_eq!(
            ServiceFailureCode::ProtocolMismatch.recovery_class(),
            ClientRecoveryClass::RetryNextRound
        );
        assert_eq!(
            ServiceFailureCode::StartFailed.recovery_class(),
            ClientRecoveryClass::RetryNextRound
        );
        assert_eq!(
            ServiceFailureCode::AccessDenied.recovery_class(),
            ClientRecoveryClass::RetryNextRound
        );
    }

    #[test]
    fn client_retry_backoff_is_bounded_and_categorized() {
        let delays = (0..CLIENT_RETRY_ATTEMPTS)
            .map(client_retry_delay)
            .collect::<Vec<_>>();
        assert_eq!(delays[0], Some(Duration::from_millis(25)));
        assert_eq!(delays[1], Some(Duration::from_millis(50)));
        assert_eq!(delays[6], Some(Duration::from_millis(1_000)));
        assert_eq!(delays[7], None);
        assert!(delays
            .iter()
            .flatten()
            .all(|delay| *delay <= CLIENT_RETRY_MAX_DELAY));

        assert_eq!(
            service_state_failure(ServiceState::Stopped).code,
            ServiceFailureCode::Stopped
        );
        assert_eq!(
            service_state_failure(ServiceState::StopPending).code,
            ServiceFailureCode::Stopped
        );
        assert_eq!(
            service_state_failure(ServiceState::Paused).code,
            ServiceFailureCode::StartFailed
        );
    }

    #[test]
    fn client_recovery_round_retries_only_transient_connection_states() {
        let mut attempts = vec![
            Err(ServiceFailure::new(ServiceFailureCode::Busy, "busy")),
            Err(ServiceFailure::new(
                ServiceFailureCode::Starting,
                "starting",
            )),
            Err(ServiceFailure::new(
                ServiceFailureCode::PipeMissing,
                "pipe missing",
            )),
            Ok(7_u32),
        ]
        .into_iter();
        let mut waits = Vec::new();
        let value = run_client_recovery_round(
            || attempts.next().expect("恢复轮进行了多余尝试"),
            |delay| waits.push(delay),
        )
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(
            waits,
            vec![
                Duration::from_millis(25),
                Duration::from_millis(50),
                Duration::from_millis(100)
            ]
        );

        let mut immediate_attempts = 0;
        let mismatch = run_client_recovery_round::<(), _, _>(
            || {
                immediate_attempts += 1;
                Err(ServiceFailure::new(
                    ServiceFailureCode::ProtocolMismatch,
                    "mismatch",
                ))
            },
            |_| panic!("协议不兼容不应在同一恢复轮睡眠重试"),
        )
        .unwrap_err();
        assert_eq!(mismatch.code, ServiceFailureCode::ProtocolMismatch);
        assert_eq!(immediate_attempts, 1);

        let mut bounded_attempts = 0;
        let mut bounded_waits = Vec::new();
        let busy = run_client_recovery_round::<(), _, _>(
            || {
                bounded_attempts += 1;
                Err(ServiceFailure::new(ServiceFailureCode::Busy, "busy"))
            },
            |delay| bounded_waits.push(delay),
        )
        .unwrap_err();
        assert_eq!(busy.code, ServiceFailureCode::Busy);
        assert_eq!(bounded_attempts, CLIENT_RETRY_ATTEMPTS);
        assert_eq!(bounded_waits.len(), CLIENT_RETRY_ATTEMPTS - 1);
    }

    #[test]
    fn repair_target_and_arguments_are_fixed_to_the_gui_sibling() {
        let gui = Path::new(r"C:\Program Files\LogCrate\logcrate.exe");
        assert_eq!(
            repair_executable_path(gui).unwrap(),
            Path::new(r"C:\Program Files\LogCrate\logcrate_index_service.exe")
        );
        assert_eq!(REPAIR_EXECUTABLE_NAME, "logcrate_index_service.exe");
        assert_eq!(REPAIR_ARGUMENTS, "--install");
    }

    #[test]
    fn repair_launch_and_exit_failures_keep_distinct_categories() {
        let cancelled =
            classify_elevation_launch_error(io::Error::from_raw_os_error(ERROR_CANCELLED));
        assert_eq!(cancelled.code, ServiceFailureCode::ElevationCancelled);

        let launch_failed =
            classify_elevation_launch_error(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED));
        assert_eq!(launch_failed.code, ServiceFailureCode::RepairFailed);

        let missing = validate_repair_executable(Path::new(
            r"C:\definitely-missing\logcrate_index_service.exe",
        ))
        .unwrap_err();
        assert_eq!(missing.code, ServiceFailureCode::RepairExecutableMissing);
        assert_eq!(
            interpret_repair_exit_code(7).unwrap_err().code,
            ServiceFailureCode::RepairFailed
        );
        assert!(interpret_repair_exit_code(0).is_ok());
    }

    #[test]
    fn handshake_failures_distinguish_protocol_from_readiness() {
        let mismatch = handshake_failure(ProtocolVersionMismatch(99).into());
        assert_eq!(mismatch.code, ServiceFailureCode::ProtocolMismatch);

        let unavailable = handshake_failure(anyhow!(io::Error::from_raw_os_error(2)));
        assert_eq!(unavailable.code, ServiceFailureCode::PipeMissing);

        let pipe_denied = pipe_open_failure(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED));
        assert_eq!(pipe_denied.code, ServiceFailureCode::AccessDenied);
        let pipe_busy = pipe_open_failure(io::Error::from_raw_os_error(ERROR_PIPE_BUSY));
        assert_eq!(pipe_busy.code, ServiceFailureCode::Busy);
        let pipe_missing = pipe_open_failure(io::Error::from_raw_os_error(ERROR_FILE_NOT_FOUND));
        assert_eq!(pipe_missing.code, ServiceFailureCode::PipeMissing);
    }

    #[test]
    fn protocol_round_trips_requests_and_bounded_batches() {
        let requests = [
            Request::Hello,
            Request::EnumerateMft { volume: 'd' },
            Request::QueryUsn { volume: 'D' },
            Request::ReadUsn {
                volume: 'D',
                start_usn: 10,
                journal_id: 20,
                target_usn: 30,
            },
        ];
        for request in requests {
            let mut bytes = Vec::new();
            write_request(&mut bytes, &request).unwrap();
            let decoded = read_request(&mut Cursor::new(bytes)).unwrap();
            let expected = match request {
                Request::EnumerateMft { .. } => Request::EnumerateMft { volume: 'D' },
                other => other,
            };
            assert_eq!(decoded, expected);
        }

        let responses = [
            Response::Hello {
                protocol: PROTOCOL_VERSION,
            },
            Response::MftBatch(vec![record()]),
            Response::Complete(MftEnumeration {
                batches: 2,
                records: 3,
                last_reference: 4,
            }),
            Response::UsnInfo(UsnJournalInfo {
                journal_id: 5,
                first_usn: 6,
                next_usn: 7,
                lowest_valid_usn: 4,
            }),
            Response::UsnBatch(vec![record()]),
            Response::UsnComplete(UsnReadSummary {
                batches: 8,
                records: 9,
                next_usn: 10,
            }),
            Response::Error {
                code: 5,
                message: "拒绝访问".into(),
            },
        ];
        for response in responses {
            let mut bytes = Vec::new();
            write_response(&mut bytes, &response).unwrap();
            assert_eq!(read_response(&mut Cursor::new(bytes)).unwrap(), response);
        }
    }

    #[test]
    fn protocol_rejects_oversized_unknown_and_trailing_data() {
        let mut oversized = Vec::new();
        oversized.extend(MAGIC);
        oversized.extend(PROTOCOL_VERSION.to_le_bytes());
        oversized.extend(REQUEST_HELLO.to_le_bytes());
        oversized.extend(((MAX_FRAME_BODY + 1) as u32).to_le_bytes());
        assert!(read_request(&mut Cursor::new(oversized)).is_err());

        let mut unknown = Vec::new();
        write_frame(&mut unknown, 999, &[]).unwrap();
        assert!(read_request(&mut Cursor::new(unknown)).is_err());

        let mut records = encode_records(&[record()]).unwrap();
        records.push(0);
        assert!(decode_records(&records).is_err());
    }

    #[test]
    fn disconnect_is_cancellation_and_active_slots_are_released() {
        let disconnect = anyhow!(io::Error::from_raw_os_error(109));
        assert!(is_disconnect(&disconnect));

        let active = AtomicUsize::new(1);
        {
            let _guard = ActiveClientGuard(&active);
            assert_eq!(active.load(Ordering::Acquire), 1);
        }
        assert_eq!(active.load(Ordering::Acquire), 0);
        assert_eq!(MAX_CONCURRENT_CLIENTS, 4);
    }

    #[test]
    fn concurrent_client_storm_is_bounded() {
        let active = AtomicUsize::new(0);
        let guards = (0..64)
            .filter_map(|_| try_acquire_client_slot(&active))
            .collect::<Vec<_>>();
        assert_eq!(guards.len(), MAX_CONCURRENT_CLIENTS);
        assert_eq!(active.load(Ordering::Acquire), MAX_CONCURRENT_CLIENTS);
        drop(guards);
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn pipe_accept_capacity_is_separate_from_business_capacity() {
        assert_eq!(MAX_CONCURRENT_CLIENTS, 4);
        assert!(MAX_PIPE_INSTANCES > MAX_CONCURRENT_CLIENTS as u32);

        let busy = Response::Error {
            code: BUSY_RESPONSE_CODE,
            message: "索引服务并发请求已达上限".into(),
        };
        let mut bytes = Vec::new();
        write_response(&mut bytes, &busy).unwrap();
        assert_eq!(read_response(&mut Cursor::new(bytes)).unwrap(), busy);
    }

    #[test]
    fn real_pipe_saturation_reconnect_disconnect_and_stop_are_bounded() {
        let pipe_name = unique_test_pipe_name();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let server_pipe_name = pipe_name.clone();
        let (finished_tx, finished_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let result = run_pipe_server_at(&server_pipe_name, &server_stop, false);
            let _ = finished_tx.send(result);
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut active_clients = (0..MAX_CONCURRENT_CLIENTS)
            .map(|_| connect_test_client(&pipe_name, deadline))
            .collect::<Vec<_>>();

        let mut extra = open_test_pipe(&pipe_name, deadline);
        assert!(matches!(
            read_test_response(&mut extra, deadline, "busy"),
            Response::Error {
                code: BUSY_RESPONSE_CODE,
                ..
            }
        ));
        drop(extra);

        let mut disconnected = active_clients.remove(0);
        disconnected.write_all(&MAGIC[..2]).unwrap();
        drop(disconnected);
        active_clients.push(connect_test_client(&pipe_name, deadline));

        drop(active_clients.remove(0));
        active_clients.push(connect_test_client(&pipe_name, deadline));

        drop(active_clients);
        stop.store(true, Ordering::SeqCst);
        wake_pipe_server_at(&pipe_name);
        let result = finished_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("索引服务未在停止唤醒后有界退出");
        result.unwrap();
        server.join().unwrap();

        assert!(OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe_name)
            .is_err());
    }

    #[test]
    fn ipc_parser_fuzz_property_never_panics_or_allocates_oversized_frames() {
        let mut state = 0x9e37_79b9_u32;
        for length in 0..2048_usize {
            let mut bytes = vec![0_u8; length];
            for byte in &mut bytes {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            let request = std::panic::catch_unwind(|| {
                let _ = read_request(&mut Cursor::new(&bytes));
            });
            let records = std::panic::catch_unwind(|| {
                let _ = decode_records(&bytes);
            });
            assert!(request.is_ok());
            assert!(records.is_ok());
        }
        assert!(PIPE_SDDL.contains(";;;IU"));
        assert!(PIPE_SDDL.contains(";;;SY"));
    }
}
