use std::ffi::c_void;
use std::ptr;
use std::slice;

use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::BI_BITFIELDS;
use windows::Win32::Graphics::Gdi::BITMAPV4HEADER;
use windows::Win32::Graphics::Gdi::BitBlt;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, GetDC, HBITMAP, HDC, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::Owned;

use super::HandleCell;
use super::error::Error;
use super::handle::Handle;

#[derive(Clone, Debug)]
pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>,
}

#[derive(Debug)]
struct DeviceContext {
    inner: HDC,
    handle: Option<HWND>,
}

impl Drop for DeviceContext {
    fn drop(&mut self) {
        unsafe {
            if self.handle.is_some() {
                let _ = ReleaseDC(self.handle, self.inner);
            } else {
                let _ = DeleteDC(self.inner);
            }
        }
    }
}

#[derive(Debug)]
struct Bitmap {
    inner: Owned<HBITMAP>,
    device_context: DeviceContext,
    width: i32,
    height: i32,
    size: usize,
    buffer: *const u8,
}

#[derive(Debug)]
pub struct Capture {
    handle: HandleCell,
    bitmap: Option<Bitmap>,
}

impl Capture {
    pub fn new(handle: Handle) -> Self {
        Self {
            handle: HandleCell::new(handle),
            bitmap: None,
        }
    }

    #[inline]
    pub fn grab(&mut self) -> Result<Frame, Error> {
        self.grab_inner()
    }

    fn grab_inner(&mut self) -> Result<Frame, Error> {
        let handle = self.handle.as_inner().ok_or(Error::WindowNotFound)?;
        let rect = get_rect(handle)?;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width == 0 || height == 0 {
            return Err(Error::WindowNotFound);
        }
        if self.bitmap.is_none() {
            self.bitmap = Some(create_bitmap(width, height)?);
        }
        let bitmap = self.bitmap.as_ref().unwrap();
        if width != bitmap.width || height != bitmap.height {
            self.bitmap = None;
            return Err(Error::InvalidWindowSize);
        }
        let bitmap_device_context = &bitmap.device_context;
        let handle_device_context = get_device_context(handle)?;
        let object = unsafe { SelectObject(bitmap_device_context.inner, (*bitmap.inner).into()) };
        if object.is_invalid() {
            return Err(Error::from_last_win_error());
        }
        let result = unsafe {
            BitBlt(
                bitmap_device_context.inner,
                0,
                0,
                bitmap.width,
                bitmap.height,
                Some(handle_device_context.inner),
                0,
                0,
                SRCCOPY,
            )
        };
        let _ = unsafe { SelectObject(bitmap_device_context.inner, object) };
        if let Err(error) = result {
            return Err(Error::from(error));
        }
        // SAFETY: I swear on the love of Axis Order, this call passed the safety vibe check
        let ptr = unsafe { slice::from_raw_parts(bitmap.buffer, bitmap.size) };
        let data = ptr.to_vec();
        Ok(Frame {
            width: bitmap.width,
            height: bitmap.height,
            data,
        })
    }
}

#[inline]
fn get_rect(handle: HWND) -> Result<RECT, Error> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(handle, &raw mut rect) }?;
    Ok(rect)
}

#[inline]
fn get_device_context(handle: HWND) -> Result<DeviceContext, Error> {
    let device_context = unsafe { GetDC(handle.into()) };
    if device_context.is_invalid() {
        return Err(Error::from_last_win_error());
    }
    Ok(DeviceContext {
        inner: device_context,
        handle: Some(handle),
    })
}

#[inline]
fn create_bitmap(width: i32, height: i32) -> Result<Bitmap, Error> {
    let device_context = unsafe { CreateCompatibleDC(None) };
    if device_context.is_invalid() {
        return Err(Error::from_last_win_error());
    }
    let size = width as usize * height as usize * 4;
    let buffer = ptr::null_mut::<c_void>();
    let info = BITMAPV4HEADER {
        bV4Size: size_of::<BITMAPV4HEADER>() as u32,
        bV4Width: width,
        bV4Height: -height,
        bV4Planes: 1,
        bV4BitCount: 32,
        bV4V4Compression: BI_BITFIELDS,
        bV4RedMask: 0x00FF0000,
        bV4GreenMask: 0x0000FF00,
        bV4BlueMask: 0x000000FF,
        ..BITMAPV4HEADER::default()
    };
    let dib = unsafe {
        CreateDIBSection(
            Some(device_context),
            (&raw const info).cast(),
            DIB_RGB_COLORS,
            (&raw const buffer).cast_mut(),
            None,
            0,
        )?
    };
    Ok(Bitmap {
        inner: unsafe { Owned::new(dib) },
        device_context: DeviceContext {
            inner: device_context,
            handle: None,
        },
        width,
        height,
        size,
        buffer: buffer.cast(),
    })
}
