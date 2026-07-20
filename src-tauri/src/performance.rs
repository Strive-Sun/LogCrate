use crate::archive::{open_archive, resolve_archive_chain};
use crate::index::{IndexProgress, SessionManager, MAX_UNCOMPRESSED};
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const LINE_BYTES: u64 = 256;

struct SyntheticLog {
    position: u64,
    length: u64,
}

impl SyntheticLog {
    fn new(length: u64) -> Self {
        Self {
            position: 0,
            length,
        }
    }
}

impl Read for SyntheticLog {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = output.len().min((self.length - self.position) as usize);
        for (offset, byte) in output[..count].iter_mut().enumerate() {
            let absolute = self.position + offset as u64;
            let column = absolute % LINE_BYTES;
            let line = absolute / LINE_BYTES;
            *byte = if column == LINE_BYTES - 1 {
                b'\n'
            } else {
                b'a' + ((line.wrapping_mul(31) + column.wrapping_mul(17)) % 26) as u8
            };
        }
        self.position += count as u64;
        Ok(count)
    }
}

#[repr(C)]
#[allow(non_snake_case)]
struct ProcessMemoryCounters {
    cb: u32,
    PageFaultCount: u32,
    PeakWorkingSetSize: usize,
    WorkingSetSize: usize,
    QuotaPeakPagedPoolUsage: usize,
    QuotaPagedPoolUsage: usize,
    QuotaPeakNonPagedPoolUsage: usize,
    QuotaNonPagedPoolUsage: usize,
    PagefileUsage: usize,
    PeakPagefileUsage: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_snake_case)]
struct FileTime {
    dwLowDateTime: u32,
    dwHighDateTime: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn GetProcessTimes(
        process: *mut c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
}

#[link(name = "psapi")]
extern "system" {
    fn GetProcessMemoryInfo(
        process: *mut c_void,
        counters: *mut ProcessMemoryCounters,
        size: u32,
    ) -> i32;
}

fn working_set_bytes() -> u64 {
    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };
    // SAFETY: both pointers reference valid storage for the duration of the call.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<ProcessMemoryCounters>() as u32,
        )
    };
    if ok == 0 {
        0
    } else {
        counters.WorkingSetSize as u64
    }
}

fn file_time_value(value: FileTime) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

fn process_cpu_100ns() -> u64 {
    let mut creation = FileTime {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    // SAFETY: all output pointers reference valid writable FileTime values.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        0
    } else {
        file_time_value(kernel) + file_time_value(user)
    }
}

struct ResourceSample {
    baseline_working_set: u64,
    peak_working_set: u64,
    cpu_100ns: u64,
}

fn sample_resources<T>(operation: impl FnOnce() -> T) -> (T, ResourceSample) {
    let baseline = working_set_bytes();
    let peak = Arc::new(AtomicU64::new(baseline));
    let stop = Arc::new(AtomicBool::new(false));
    let sample_peak = peak.clone();
    let sample_stop = stop.clone();
    let cpu_before = process_cpu_100ns();
    let sampler = std::thread::spawn(move || {
        while !sample_stop.load(Ordering::Acquire) {
            sample_peak.fetch_max(working_set_bytes(), Ordering::AcqRel);
            std::thread::sleep(Duration::from_millis(10));
        }
        sample_peak.fetch_max(working_set_bytes(), Ordering::AcqRel);
    });
    let result = operation();
    let cpu_after = process_cpu_100ns();
    stop.store(true, Ordering::Release);
    sampler.join().unwrap();
    (
        result,
        ResourceSample {
            baseline_working_set: baseline,
            peak_working_set: peak.load(Ordering::Acquire),
            cpu_100ns: cpu_after.saturating_sub(cpu_before),
        },
    )
}

fn write_plain(path: &Path, size: u64) -> anyhow::Result<()> {
    let mut output = File::create(path)?;
    io::copy(&mut SyntheticLog::new(size), &mut output)?;
    output.flush()?;
    Ok(())
}

fn write_zip(path: &Path, size: u64) -> anyhow::Result<()> {
    let file = File::create(path)?;
    let mut archive = zip::ZipWriter::new(file);
    archive.start_file(
        "performance.log",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )?;
    io::copy(&mut SyntheticLog::new(size), &mut archive)?;
    archive.finish()?;
    Ok(())
}

fn append_tar<W: Write>(writer: W, size: u64) -> anyhow::Result<W> {
    let mut archive = tar::Builder::new(writer);
    let mut header = tar::Header::new_gnu();
    header.set_size(size);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append_data(&mut header, "performance.log", SyntheticLog::new(size))?;
    archive.finish()?;
    Ok(archive.into_inner()?)
}

fn write_tar_gzip(path: &Path, size: u64) -> anyhow::Result<()> {
    let encoder =
        flate2::write::GzEncoder::new(File::create(path)?, flate2::Compression::default());
    append_tar(encoder, size)?.finish()?;
    Ok(())
}

fn write_tar_zstd(path: &Path, size: u64) -> anyhow::Result<()> {
    let encoder = zstd::stream::write::Encoder::new(File::create(path)?, 3)?;
    append_tar(encoder, size)?.finish()?;
    Ok(())
}

fn write_nested_zip(path: &Path, inner: &Path) -> anyhow::Result<()> {
    let file = File::create(path)?;
    let mut archive = zip::ZipWriter::new(file);
    archive.start_file("inner.tar.gz", zip::write::SimpleFileOptions::default())?;
    io::copy(&mut File::open(inner)?, &mut archive)?;
    archive.finish()?;
    Ok(())
}

fn directory_bytes(path: &Path) -> u64 {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

struct BaselineResult {
    name: String,
    archive_bytes: u64,
    decoded_bytes: u64,
    elapsed: Duration,
    first_progress: Duration,
    lines: u64,
    cache_bytes: u64,
    baseline_working_set: u64,
    peak_working_set: u64,
    cpu_100ns: u64,
}

fn run_case(
    name: &str,
    chain: &str,
    decoded_bytes: u64,
    cache_root: &Path,
) -> anyhow::Result<BaselineResult> {
    let case_cache = cache_root.join(name);
    fs::create_dir_all(&case_cache)?;
    let started = Instant::now();
    let ((first_progress, lines, cache_bytes), resources) = sample_resources(|| {
        let resolved = resolve_archive_chain(chain, &case_cache).unwrap();
        let archive_bytes = fs::metadata(resolved.path()).unwrap().len();
        let mut archive = open_archive(resolved.path()).unwrap();
        let entry = archive
            .entries()
            .unwrap()
            .into_iter()
            .find(|entry| entry.is_log)
            .unwrap();
        let mut stream = archive.open_entry(&entry.path).unwrap();
        let sessions = SessionManager::default();
        let session_cache = case_cache.join("sessions");
        sessions.set_cache_dir(session_cache.clone());
        let opened = sessions
            .prepare(format!("{name} › {}", entry.path), decoded_bytes)
            .unwrap();
        let index_started = Instant::now();
        let mut first = None;
        let mut failure = None;
        sessions.index_with_limit(
            &opened.session_id,
            decoded_bytes,
            &mut stream,
            MAX_UNCOMPRESSED,
            |event: IndexProgress| {
                if first.is_none() && event.indexed_lines > 0 {
                    first = Some(index_started.elapsed());
                }
                if event.failed {
                    failure = event.error;
                }
            },
        );
        assert!(failure.is_none(), "indexing failed: {failure:?}");
        let lines = sessions.line_count(&opened.session_id);
        let cache_bytes = directory_bytes(&session_cache);
        sessions.close(&opened.session_id);
        assert_eq!(directory_bytes(&session_cache), 0, "session cache leaked");
        drop(stream);
        drop(archive);
        drop(resolved);
        let nested_leftovers = fs::read_dir(&case_cache)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("nested-"))
            .count();
        assert_eq!(nested_leftovers, 0, "nested cache leaked");
        (
            first.unwrap_or(index_started.elapsed()),
            lines,
            cache_bytes.max(archive_bytes),
        )
    });
    let root_archive = chain.split("::").next().unwrap();
    Ok(BaselineResult {
        name: name.to_string(),
        archive_bytes: fs::metadata(root_archive)?.len(),
        decoded_bytes,
        elapsed: started.elapsed(),
        first_progress,
        lines,
        cache_bytes,
        baseline_working_set: resources.baseline_working_set,
        peak_working_set: resources.peak_working_set,
        cpu_100ns: resources.cpu_100ns,
    })
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn render_report(size_mib: u64, results: &[BaselineResult]) -> String {
    let mut report = format!(
        "# Windows 归档性能基线\n\n- 日期：{}\n- 测试数据：{} MiB/条目\n- 逻辑处理器：{}\n- 构建：Rust release test harness\n\n| 场景 | 归档 MiB | 解码 MiB | 首批行 ms | 总耗时 s | 吞吐 MiB/s | CPU 单核占用 | 内存增量 MiB | 缓存峰值 MiB | 行数 |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        std::env::var("LOGCRATE_PERF_DATE").unwrap_or_else(|_| "local".into()),
        size_mib,
        std::thread::available_parallelism().map(usize::from).unwrap_or(1),
    );
    for result in results {
        let seconds = result.elapsed.as_secs_f64().max(0.001);
        let cpu_seconds = result.cpu_100ns as f64 / 10_000_000.0;
        let memory_delta = result
            .peak_working_set
            .saturating_sub(result.baseline_working_set);
        report.push_str(&format!(
            "| {} | {:.2} | {:.2} | {:.1} | {:.2} | {:.1} | {:.0}% | {:.2} | {:.2} | {} |\n",
            result.name,
            mib(result.archive_bytes),
            mib(result.decoded_bytes),
            result.first_progress.as_secs_f64() * 1000.0,
            seconds,
            mib(result.decoded_bytes) / seconds,
            cpu_seconds / seconds * 100.0,
            mib(memory_delta),
            mib(result.cache_bytes),
            result.lines,
        ));
    }
    report.push_str(
        "\n> CPU 单核占用 100% 表示约占满一个逻辑处理器；缓存峰值包含索引缓存或当前嵌套中间归档的较大者。\n",
    );
    report
}

#[test]
#[ignore = "run with scripts/windows-archive-baseline.ps1"]
fn windows_archive_performance_baseline() {
    let size_mib = std::env::var("LOGCRATE_PERF_MIB")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(256);
    let decoded_bytes = size_mib * 1024 * 1024;
    assert!(decoded_bytes <= MAX_UNCOMPRESSED);
    let root =
        std::env::temp_dir().join(format!("logcrate-windows-baseline-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let plain = root.join("performance.log");
    let zip = root.join("performance.zip");
    let tar_gzip = root.join("performance.tar.gz");
    let tar_zstd = root.join("performance.tar.zst");
    let nested = root.join("nested.zip");
    write_plain(&plain, decoded_bytes).unwrap();
    write_zip(&zip, decoded_bytes).unwrap();
    write_tar_gzip(&tar_gzip, decoded_bytes).unwrap();
    write_tar_zstd(&tar_zstd, decoded_bytes).unwrap();
    write_nested_zip(&nested, &tar_gzip).unwrap();

    let cache = root.join("cache");
    let cases = [
        ("plain", plain.to_string_lossy().into_owned()),
        ("zip-deflate", zip.to_string_lossy().into_owned()),
        ("tar-gzip", tar_gzip.to_string_lossy().into_owned()),
        ("tar-zstd", tar_zstd.to_string_lossy().into_owned()),
        (
            "nested-zip-tar-gzip",
            format!("{}::inner.tar.gz", nested.to_string_lossy()),
        ),
    ];
    let max_memory_mib = std::env::var("LOGCRATE_PERF_MAX_MEMORY_MIB")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(192);
    let mut results = Vec::new();
    for (name, chain) in cases {
        let result = run_case(name, &chain, decoded_bytes, &cache).unwrap();
        let memory_delta = result
            .peak_working_set
            .saturating_sub(result.baseline_working_set);
        assert!(
            memory_delta <= max_memory_mib * 1024 * 1024,
            "{name} memory delta {:.2} MiB exceeds {max_memory_mib} MiB",
            mib(memory_delta)
        );
        assert_eq!(result.cache_bytes, decoded_bytes);
        assert!(result.lines >= decoded_bytes / LINE_BYTES - 1);
        results.push(result);
    }

    let report = render_report(size_mib, &results);
    println!("\n{report}");
    if let Ok(path) = std::env::var("LOGCRATE_PERF_REPORT") {
        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, &report).unwrap();
    }
    fs::remove_dir_all(&root).unwrap();
}
