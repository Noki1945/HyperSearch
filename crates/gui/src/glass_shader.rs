//! Experimental shader compilation only; the live DWM backend does not use this shader.
//! Shader validity is checked separately from visual/backend acceptance.
use windows::{
    Win32::Graphics::Direct3D::{Fxc::*, ID3DBlob, ID3DInclude},
    core::s,
};
pub const SOURCE: &str = include_str!("glass.hlsl");
pub fn compile() -> Result<ID3DBlob, String> {
    let mut code = None;
    let mut errors = None;
    unsafe {
        D3DCompile(
            SOURCE.as_ptr().cast(),
            SOURCE.len(),
            s!("hypersearch-glass"),
            None,
            None::<&ID3DInclude>,
            s!("main"),
            s!("ps_5_0"),
            D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
        .map_err(|error| {
            errors
                .map(|b| {
                    String::from_utf8_lossy(std::slice::from_raw_parts(
                        b.GetBufferPointer().cast(),
                        b.GetBufferSize(),
                    ))
                    .trim_end_matches('\0')
                    .to_owned()
                })
                .unwrap_or_else(|| error.to_string())
        })?;
    }
    code.ok_or_else(|| "D3DCompile returned no pixel shader".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refraction_shader_compiles_for_shader_model_5() {
        let shader = compile().expect("glass shader compile");
        assert!(unsafe { shader.GetBufferSize() } > 0);
    }
}
