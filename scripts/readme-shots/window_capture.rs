use std::path::Path;
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat};
use windows::Graphics::Imaging::{BitmapBounds, BitmapEncoder, SoftwareBitmap};
use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}

struct Capture {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

/// Capture this HWND's composed surface, including Windows clipping/alpha.
/// No desktop pixels, fabricated corner masks or rescaling enter the PNG.
pub(super) fn save(window: &tauri::WebviewWindow, path: &Path) -> Result<(i32, i32), String> {
    let capture = || -> windows::core::Result<(i32, i32)> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        let _apartment = Apartment;
        println!("Capture: initialize WinRT");
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                0x80004001u32 as i32,
            )));
        }
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let hwnd = window
            .hwnd()
            .map_err(|_| windows::core::Error::from_win32())?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(HWND(hwnd.0))? };
        println!("Capture: own HWND item created");
        let mut d3d = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
        }
        let dxgi: IDXGIDevice = d3d.unwrap().cast()?;
        let device: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            item.Size()?,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        let capture = Capture { session, pool };
        capture.session.SetIsCursorCaptureEnabled(false)?;
        capture.session.StartCapture()?;
        println!("Capture: frame session started");
        let deadline = Instant::now() + Duration::from_secs(5);
        let frame = loop {
            match capture.pool.TryGetNextFrame() {
                Ok(frame) => break frame,
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        let size = frame.ContentSize()?;
        println!("Capture: received frame {}x{}", size.Width, size.Height);
        let bitmap = SoftwareBitmap::CreateCopyFromSurfaceAsync(&frame.Surface()?)?.get()?;
        println!("Capture: frame copied to CPU");
        let stream = InMemoryRandomAccessStream::new()?;
        let encoder = BitmapEncoder::CreateAsync(BitmapEncoder::PngEncoderId()?, &stream)?.get()?;
        encoder.SetSoftwareBitmap(&bitmap)?;
        encoder.BitmapTransform()?.SetBounds(BitmapBounds {
            X: 0,
            Y: 0,
            Width: size.Width as u32,
            Height: size.Height as u32,
        })?;
        encoder.FlushAsync()?.get()?;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        let length = stream.Size()? as u32;
        reader.LoadAsync(length)?.get()?;
        let mut bytes = vec![0; length as usize];
        reader.ReadBytes(&mut bytes)?;
        std::fs::write(path, bytes).map_err(|_| windows::core::Error::from_win32())?;
        Ok((size.Width, size.Height))
    };
    capture().map_err(|e| format!("Windows Graphics Capture failed: {e}"))
}
