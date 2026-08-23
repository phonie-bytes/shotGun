// src/dxgi_capture.rs
//! Direct DXGI Desktop Duplication access, bypassing xcap's `video_recorder()`.
//!
//! xcap 0.2.2's `ImplVideoRecorder::new()` creates its D3D11 device against
//! whatever adapter `D3D11CreateDevice(None, ...)` picks as "default," then
//! searches only *that* adapter's outputs for the target monitor. On any
//! multi-GPU/hybrid-graphics system where the target monitor is attached to
//! a different adapter than the default one, that search comes up empty and
//! fails with `DXGI_ERROR_NOT_FOUND`. This module does the search properly:
//! enumerate every adapter's every output to find the one that owns the
//! target monitor, *then* create the D3D11 device against that specific
//! adapter.

use windows::core::Interface;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11CreateDevice,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};

pub struct DuplicationSession {
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
}

/// Opens a duplication session for the monitor containing point (x, y) —
/// pass a point known to be inside the target monitor's bounds (e.g. its
/// top-left corner). Correctly searches every adapter's outputs, unlike
/// xcap's `video_recorder()`.
pub fn open_for_point(x: i32, y: i32) -> Result<DuplicationSession, String> {
    unsafe {
        let hmonitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        if hmonitor.is_invalid() {
            return Err("MonitorFromPoint found no monitor at the target coordinates".to_string());
        }

        let factory: IDXGIFactory1 =
            CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1 failed: {e}"))?;

        let mut adapter_index = 0u32;
        loop {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_index) {
                Ok(a) => a,
                Err(_) => {
                    return Err(
                        "No adapter on this system exposes the target monitor (enumerated all adapters/outputs without a match)".to_string(),
                    )
                }
            };
            adapter_index += 1;

            let mut output_index = 0u32;
            loop {
                let output = match adapter.EnumOutputs(output_index) {
                    Ok(o) => o,
                    Err(_) => break, // no more outputs on this adapter; try the next adapter
                };
                output_index += 1;

                let desc = output
                    .GetDesc()
                    .map_err(|e| format!("IDXGIOutput::GetDesc failed: {e}"))?;
                if desc.Monitor != hmonitor {
                    continue;
                }

                // Found the adapter+output that owns this monitor. Create
                // the D3D11 device against THIS specific adapter (required:
                // driver type must be UNKNOWN whenever an explicit adapter
                // is passed).
                let adapter_as_dxgi: IDXGIAdapter = adapter
                    .cast()
                    .map_err(|e| format!("IDXGIAdapter cast failed: {e}"))?;

                let mut d3d_device: Option<ID3D11Device> = None;
                let mut d3d_context: Option<ID3D11DeviceContext> = None;
                D3D11CreateDevice(
                    Some(&adapter_as_dxgi),
                    D3D_DRIVER_TYPE_UNKNOWN,
                    None,
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut d3d_device),
                    None,
                    Some(&mut d3d_context),
                )
                .map_err(|e| format!("D3D11CreateDevice failed: {e}"))?;

                let d3d_device =
                    d3d_device.ok_or_else(|| "D3D11CreateDevice returned no device".to_string())?;
                let d3d_context = d3d_context
                    .ok_or_else(|| "D3D11CreateDevice returned no immediate context".to_string())?;

                let dxgi_device: IDXGIDevice = d3d_device
                    .cast()
                    .map_err(|e| format!("IDXGIDevice cast failed: {e}"))?;
                let output1: IDXGIOutput1 = output
                    .cast()
                    .map_err(|e| format!("IDXGIOutput1 cast failed: {e}"))?;
                let duplication = output1
                    .DuplicateOutput(&dxgi_device)
                    .map_err(|e| format!("DuplicateOutput failed: {e}"))?;

                return Ok(DuplicationSession {
                    d3d_device,
                    d3d_context,
                    duplication,
                });
            }
        }
    }
}

/// Acquires the next available frame, waiting up to `timeout_ms`. Returns
/// `Ok(None)` on a normal timeout / no-new-content tick (not an error —
/// the caller should just try again), `Ok(Some((width, height, rgba)))` on
/// a captured frame (tightly packed RGBA8, row-pitch padding stripped),
/// and `Err` only for a genuine duplication failure.
pub fn acquire_frame(session: &DuplicationSession, timeout_ms: u32) -> Result<Option<(u32, u32, Vec<u8>)>, String> {
    unsafe {
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;

        if let Err(e) = session.duplication.AcquireNextFrame(timeout_ms, &mut frame_info, &mut resource) {
            let _ = session.duplication.ReleaseFrame();
            if e.code() == DXGI_ERROR_WAIT_TIMEOUT {
                return Ok(None);
            }
            return Err(format!("AcquireNextFrame failed: {e}"));
        }

        if frame_info.LastPresentTime == 0 {
            let _ = session.duplication.ReleaseFrame();
            return Ok(None);
        }

        let Some(resource) = resource else {
            let _ = session.duplication.ReleaseFrame();
            return Err("AcquireNextFrame reported success but returned no resource".to_string());
        };

        let result = copy_resource_to_rgba(&session.d3d_device, &session.d3d_context, resource);
        let _ = session.duplication.ReleaseFrame();
        result.map(Some)
    }
}

unsafe fn copy_resource_to_rgba(
    d3d_device: &ID3D11Device,
    d3d_context: &ID3D11DeviceContext,
    resource: IDXGIResource,
) -> Result<(u32, u32, Vec<u8>), String> {
    let source_texture: ID3D11Texture2D = resource
        .cast()
        .map_err(|e| format!("Resource -> ID3D11Texture2D cast failed: {e}"))?;

    let mut source_desc = D3D11_TEXTURE2D_DESC::default();
    source_texture.GetDesc(&mut source_desc);
    source_desc.BindFlags = 0;
    source_desc.MiscFlags = 0;
    source_desc.Usage = D3D11_USAGE_STAGING;
    source_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;

    let mut copy_texture = None;
    d3d_device
        .CreateTexture2D(&source_desc, None, Some(&mut copy_texture))
        .map_err(|e| format!("CreateTexture2D failed: {e}"))?;
    let copy_texture = copy_texture.ok_or_else(|| "CreateTexture2D returned no texture".to_string())?;

    let dst_resource: ID3D11Resource = copy_texture
        .cast()
        .map_err(|e| format!("Staging texture cast failed: {e}"))?;
    let src_resource: ID3D11Resource = source_texture
        .cast()
        .map_err(|e| format!("Source texture cast failed: {e}"))?;
    d3d_context.CopyResource(Some(&dst_resource), Some(&src_resource));

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    d3d_context
        .Map(Some(&dst_resource), 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        .map_err(|e| format!("Map failed: {e}"))?;

    let width = source_desc.Width;
    let height = source_desc.Height;
    let row_pitch = mapped.RowPitch as usize;
    let row_bytes = (width as usize) * 4;

    // Copy row-by-row and swap BGRA -> RGBA, stripping any row-pitch
    // padding the GPU added beyond `width * 4` bytes per row.
    let mut rgba = Vec::with_capacity(row_bytes * height as usize);
    let base = mapped.pData as *const u8;
    for row in 0..height as usize {
        let row_slice = std::slice::from_raw_parts(base.add(row * row_pitch), row_bytes);
        for px in row_slice.chunks_exact(4) {
            rgba.push(px[2]);
            rgba.push(px[1]);
            rgba.push(px[0]);
            rgba.push(px[3]);
        }
    }

    d3d_context.Unmap(Some(&dst_resource), 0);

    Ok((width, height, rgba))
}
