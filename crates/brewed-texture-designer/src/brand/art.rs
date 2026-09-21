// The application icon: `assets/logo.svg`, drawn at the sizes the system asks for. The
// window, the taskbar and the executable (build.rs includes this file) all use it.

/// Sizes embedded in the Windows executable (used by build.rs and the tests).
#[allow(dead_code)]
pub const ICON_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

/// The logo, as it was drawn.
const LOGO: &[u8] = include_bytes!("../../assets/logo.svg");

/// RGBA pixels (straight alpha) of the icon at `size`×`size`.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    use resvg::{tiny_skia, usvg};
    let size = size.max(1);
    let blank = || vec![0; (size * size * 4) as usize];
    // The tests draw the logo at every size: a file that does not parse fails them, and an
    // icon that cannot be drawn is left empty rather than taking the window down.
    let Ok(tree) = usvg::Tree::from_data(LOGO, &usvg::Options::default()) else { return blank() };
    let Some(mut pixmap) = tiny_skia::Pixmap::new(size, size) else { return blank() };
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    // The pixmap is premultiplied; the icon formats want straight alpha.
    pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}
