#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::ffi::CString;
use alloc::format;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::iter::{self, FusedIterator};

use fs101::pid_t;
use rustix::buffer::spare_capacity;
use rustix::fd::OwnedFd;
use rustix::fs::{CWD, Dir, Mode, OFlags, openat, readlinkat};
use rustix::io::{self, Errno};

const PROC: &CStr = c"/proc";
const CHUNK: usize = 4096;

/// Implement this function to list processes. It must call
/// fs101::report_process() for each running process. If an error occurs
/// when accessing a file or a directory, it must call fs101::report_error().
pub fn ps() {
    match pids() {
        Ok(pids) => pids
            .map(|pid| pid.and_then(Process::read))
            .for_each(|process| match process {
                Ok(process) => process.report(),
                Err(failure) => failure.report(),
            }),
        Err(failure) => failure.report(),
    }
}

struct Failure {
    path: CString,
    errno: Errno,
}

impl Failure {
    fn report(&self) {
        fs101::report_error(&self.path, self.errno.raw_os_error());
    }
}

type Result<T> = core::result::Result<T, Failure>;

trait At<T> {
    fn at(self, path: &CStr) -> Result<T>;
}

impl<T> At<T> for core::result::Result<T, Errno> {
    fn at(self, path: &CStr) -> Result<T> {
        self.map_err(|errno| Failure {
            path: path.into(),
            errno,
        })
    }
}

fn pids() -> Result<impl Iterator<Item = Result<pid_t>>> {
    let fd = openat(CWD, PROC, OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty()).at(PROC)?;
    Ok(Dir::new(fd).at(PROC)?.filter_map(|entry| {
        entry
            .map(|entry| parse_pid(entry.file_name()))
            .at(PROC)
            .transpose()
    }))
}

fn parse_pid(name: &CStr) -> Option<pid_t> {
    let name = name.to_str().ok()?;
    name.bytes().all(|b| b.is_ascii_digit()).then_some(name)?.parse().ok()
}

struct Process {
    pid: pid_t,
    exe: CString,
    argv: Vec<CString>,
    envp: Vec<CString>,
}

impl Process {
    fn read(pid: pid_t) -> Result<Self> {
        let path = |file: &str| {
            CString::new(format!("/proc/{pid}/{file}")).expect("a /proc path has no NUL bytes")
        };
        let exe = path("exe");

        Ok(Self {
            pid,
            exe: readlinkat(CWD, &exe, Vec::new()).at(&exe)?,
            argv: entries(&path("cmdline"))?.collect::<Result<_>>()?,
            envp: entries(&path("environ"))?.collect::<Result<_>>()?,
        })
    }

    fn report(&self) {
        fs101::report_process(self.pid, &self.exe, &borrow(&self.argv), &borrow(&self.envp));
    }
}

fn borrow(strings: &[CString]) -> Vec<&CStr> {
    strings.iter().map(CString::as_c_str).collect()
}

fn entries(path: &CStr) -> Result<impl Iterator<Item = Result<CString>>> {
    let mut bytes = bytes(path)?.peekable();
    Ok(iter::from_fn(move || {
        bytes.peek()?;
        Some(
            bytes
                .by_ref()
                .take_while(|byte| !matches!(byte, Ok(0)))
                .collect::<Result<Vec<u8>>>()
                .map(|entry| CString::new(entry).expect("entries stop at the first NUL")),
        )
    }))
}

fn bytes(path: &CStr) -> Result<impl Iterator<Item = Result<u8>>> {
    Ok(Chunks::open(path)?.flat_map(|chunk| {
        let (bytes, failure) = match chunk {
            Ok(bytes) => (bytes, None),
            Err(failure) => (Vec::new(), Some(failure)),
        };
        bytes.into_iter().map(Ok).chain(failure.map(Err))
    }))
}

struct Chunks {
    fd: Option<OwnedFd>,
    path: CString,
}

impl Chunks {
    fn open(path: &CStr) -> Result<Self> {
        let fd = openat(CWD, path, OFlags::RDONLY, Mode::empty()).at(path)?;
        Ok(Self { fd: Some(fd), path: path.into() })
    }
}

impl Iterator for Chunks {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        let fd = self.fd.as_ref()?;
        let mut chunk = Vec::with_capacity(CHUNK);
        match io::retry_on_intr(|| io::read(fd, spare_capacity(&mut chunk))).at(&self.path) {
            Ok(1..) => Some(Ok(chunk)),
            end => {
                self.fd = None;
                end.err().map(Err)
            }
        }
    }
}

impl FusedIterator for Chunks {}
