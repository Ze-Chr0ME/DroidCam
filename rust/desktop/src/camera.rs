use anyhow::{bail, Result};
use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{Memory::*, Threading::*},
};
pub const CAPACITY: usize = 3840 * 2160 * 4 * 2;
pub struct Camera {
    mutex: HANDLE,
    want: HANDLE,
    sent: HANDLE,
    mapping: HANDLE,
    view: *mut u8,
}
fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}
impl Camera {
    pub fn new(suffix: &str) -> Result<Self> {
        unsafe {
            let mut c = Self {
                mutex: null_mut(),
                want: null_mut(),
                sent: null_mut(),
                mapping: null_mut(),
                view: null_mut(),
            };
            c.mutex = CreateMutexW(
                null(),
                0,
                wide(&format!("UnityCapture_Mutx{suffix}")).as_ptr(),
            );
            c.want = CreateEventW(
                null(),
                0,
                0,
                wide(&format!("UnityCapture_Want{suffix}")).as_ptr(),
            );
            c.sent = CreateEventW(
                null(),
                0,
                0,
                wide(&format!("UnityCapture_Sent{suffix}")).as_ptr(),
            );
            c.mapping = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                null(),
                PAGE_READWRITE,
                0,
                (CAPACITY + 32) as u32,
                wide(&format!("UnityCapture_Data{suffix}")).as_ptr(),
            );
            if [c.mutex, c.want, c.sent, c.mapping]
                .iter()
                .any(|h| h.is_null())
            {
                bail!("{}", std::io::Error::last_os_error());
            }
            c.view = MapViewOfFile(c.mapping, FILE_MAP_WRITE, 0, 0, CAPACITY + 32).Value as *mut u8;
            if c.view.is_null() {
                bail!("{}", std::io::Error::last_os_error());
            }
            Ok(c)
        }
    }
    pub fn send(&mut self, rgba: &[u8], w: usize, h: usize) -> Result<()> {
        unsafe {
            if w == 0 || h == 0 || rgba.len() != w * h * 4 || rgba.len() > CAPACITY {
                bail!("Invalid webcam frame");
            }
            let result = WaitForSingleObject(self.mutex, 1000);
            if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED {
                bail!("Webcam mapping busy");
            }
            let header = [CAPACITY as i32, w as i32, h as i32, w as i32, 0, 1, 0, 1000];
            std::ptr::copy_nonoverlapping(header.as_ptr() as *const u8, self.view, 32);
            let dst = std::slice::from_raw_parts_mut(self.view.add(32), rgba.len());
            camerabridge_core::bottom_up_copy(rgba, w, dst);
            ReleaseMutex(self.mutex);
            self.heartbeat()
        }
    }
    pub fn heartbeat(&self) -> Result<()> {
        unsafe {
            if SetEvent(self.sent) == 0 {
                bail!("Webcam signal failed");
            }
            WaitForSingleObject(self.want, 0);
        }
        Ok(())
    }
    pub fn pixel(&self, x: usize, y: usize) -> Result<[u8; 4]> {
        unsafe {
            let result = WaitForSingleObject(self.mutex, 1000);
            if result != WAIT_OBJECT_0 && result != WAIT_ABANDONED {
                bail!("Webcam mapping busy");
            }
            let header = std::slice::from_raw_parts(self.view as *const i32, 8);
            let (w, h) = (header[1] as usize, header[2] as usize);
            let valid = w <= 1280 && h <= 720 && x < w && y < h;
            let mut pixel = [0; 4];
            if valid {
                pixel.copy_from_slice(std::slice::from_raw_parts(
                    self.view.add(32 + (y * w + x) * 4),
                    4,
                ));
            }
            ReleaseMutex(self.mutex);
            if !valid {
                bail!("Invalid webcam sample");
            }
            Ok(pixel)
        }
    }
}
impl Drop for Camera {
    fn drop(&mut self) {
        unsafe {
            if !self.view.is_null() {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.view as _,
                });
            }
            for h in [self.mapping, self.sent, self.want, self.mutex] {
                if !h.is_null() {
                    CloseHandle(h);
                }
            }
        }
    }
}
