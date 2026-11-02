#![no_std]
extern crate alloc;

use alloc::format;
use core::{ffi::*, ptr, slice};
use fs101::fuse::*;
use libc::*;

/// Implement this function to to mount a "helloworld" FUSE filesystem to
/// `mntp`. The root of a filesystem must contain the following entries:
///
/// ```text
/// $ ls -lha /helloworld
/// drwxrwxr-x. 2 artem artem 19 Oct  6 05:39 .
/// drwxr-xr-x. 4 artem artem 99 Oct  6 05:39 ..
/// -r--------. 1 artem artem  0 Oct  6 05:39 hello
/// ```
///
/// The owner UID and GID of "hello" must be reported as the effective UID
/// and GID of a process that lists the directory. For example,
///
/// ```text
/// $ sudo ls -lha /helloworld
/// drwxrwxr-x. 2 root root 19 Oct  6 05:39 .
/// drwxr-xr-x. 4 root root 99 Oct  6 05:39 ..
/// -r--------. 1 root root  0 Oct  6 05:39 hello
/// ```
///
/// The content of "hello" must be (without quotes) "hello, ${PID}" where
/// PID is the id of a process that reads "hello". It is OK to report the
/// size of "hello" that does not match the content of it.
pub fn helloworld(mntp: &CStr) -> c_int {
    let ops = fuse_operations {
        getattr: Some(getattr),
        open: Some(open),
        read: Some(read),
        readdir: Some(readdir),
        ..fuse_operations::EMPTY
    };
    fuse_main(
        &[c"helloworld", c"-f", c"-o", c"ro", mntp],
        &ops,
        ptr::null_mut(),
    )
}

unsafe fn path<'a>(p: *const c_char) -> &'a [u8] {
    unsafe { CStr::from_ptr(p).to_bytes() }
}

unsafe extern "C" fn getattr(p: *const c_char, st: *mut stat, _: *mut fuse_file_info) -> c_int {
    let (mode, nlink) = match unsafe { path(p) } {
        b"/" => (S_IFDIR | 0o775, 2),
        b"/hello" => (S_IFREG | 0o400, 1),
        _ => return -ENOENT,
    };
    unsafe {
        let (st, ctx) = (&mut *st, &*fuse_get_context());
        (st.st_mode, st.st_nlink, st.st_uid, st.st_gid) = (mode, nlink, ctx.uid, ctx.gid);
    }
    0
}

unsafe extern "C" fn open(p: *const c_char, fi: *mut fuse_file_info) -> c_int {
    match (unsafe { path(p) }, unsafe { (*fi).flags } & O_ACCMODE) {
        (b"/hello", O_RDONLY) => {
            unsafe { (*fi).set_direct_io(true) };
            0
        }
        (b"/hello", _) => -EROFS,
        _ => -ENOENT,
    }
}

unsafe extern "C" fn read(
    p: *const c_char,
    buf: *mut c_char,
    len: usize,
    off: off_t,
    _: *mut fuse_file_info,
) -> c_int {
    if unsafe { path(p) } != b"/hello" {
        return -ENOENT;
    }
    let s = format!("hello, {}\n", unsafe { (*fuse_get_context()).pid });
    let data = s.as_bytes().get(off as usize..).unwrap_or(&[]);
    let n = data.len().min(len);
    unsafe { slice::from_raw_parts_mut(buf.cast(), n).copy_from_slice(&data[..n]) };
    n as c_int
}

unsafe extern "C" fn readdir(
    p: *const c_char,
    buf: *mut c_void,
    fill: fuse_fill_dir_t,
    _: off_t,
    _: *mut fuse_file_info,
    _: c_uint,
) -> c_int {
    if unsafe { path(p) } != b"/" {
        return -ENOENT;
    }

    let Some(fill) = fill else {
        return -EIO;
    };
    for s in [c".", c"..", c"hello"] {
        unsafe { fill(buf, s.as_ptr(), ptr::null(), 0, 0) };
    }
    0
}
