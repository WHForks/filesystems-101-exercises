#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::borrow::ToOwned;
use alloc::ffi::CString;
use alloc::format;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::str::FromStr;

use rustix::fs::{CWD, Dir, Mode, OFlags, openat, readlinkat};
use rustix::io::Errno;

const PROC: &CStr = c"/proc";

/// Implement this function to list open files. It must call
/// fs101::report_file() for each open file. If an error occurs when
/// accessing a file or a directory, it must call fs101::report_error().
pub fn lsof() {
    match numbered_entries::<u32>(PROC) {
        Ok(pids) => pids
            .filter_map(|pid| pid.map_err(|failure| failure.report()).ok())
            .for_each(report_open_files),
        Err(failure) => failure.report(),
    }
}

fn report_open_files(pid: u32) {
    let fd_dir = cstring(format!("/proc/{pid}/fd"));
    match numbered_entries::<u32>(&fd_dir) {
        Ok(fds) => {
            fds.map(|fd| fd.and_then(|fd| target(pid, fd)))
                .for_each(|target| match target {
                    Ok(target) => fs101::report_file(&target),
                    Err(failure) => failure.report(),
                })
        }
        Err(failure) => failure.report(),
    }
}

fn target(pid: u32, fd: u32) -> Result<CString> {
    let link = cstring(format!("/proc/{pid}/fd/{fd}"));
    readlinkat(CWD, &link, Vec::new()).at(&link)
}

fn cstring(path: alloc::string::String) -> CString {
    CString::new(path).expect("a /proc path has no NUL bytes")
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

fn numbered_entries<T: FromStr>(dir: &CStr) -> Result<impl Iterator<Item = Result<T>>> {
    let fd = openat(CWD, dir, OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty()).at(dir)?;

    let path: CString = dir.to_owned();
    Ok(Dir::new(fd).at(dir)?.filter_map(move |entry| {
        entry
            .map(|entry| parse_number(entry.file_name()))
            .at(&path)
            .transpose()
    }))
}

fn parse_number<T: FromStr>(name: &CStr) -> Option<T> {
    let name = name.to_str().ok()?;
    name.bytes()
        .all(|b| b.is_ascii_digit())
        .then_some(name)?
        .parse()
        .ok()
}
